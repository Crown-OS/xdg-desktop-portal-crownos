//! Dmabufs for PipeWire buffers, allocated on the GPU through GBM.

use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::os::fd::OwnedFd;
use std::path::PathBuf;

use anyhow::{Context, anyhow};
use gbm::{BufferObject, BufferObjectFlags, Format, Modifier};

use super::formats::IMPLICIT_MODIFIER;

const DRI_DIRECTORY: &str = "/dev/dri";
const RENDER_NODE_PREFIX: &str = "renderD";

pub(super) struct Plane {
    pub(super) fd: OwnedFd,
    pub(super) offset: u32,
    pub(super) stride: u32,
}

pub(super) struct Allocation {
    pub(super) planes: Vec<Plane>,
    /// Bytes in the whole buffer object, which every plane lies within.
    pub(super) size: u32,
    pub(super) modifier: u64,
    _object: BufferObject<()>,
}

pub(super) struct Allocator {
    device: gbm::Device<File>,
}

impl Allocator {
    pub(super) fn open() -> anyhow::Result<Self> {
        let node = first_render_node()?;
        let file = File::options()
            .read(true)
            .write(true)
            .open(&node)
            .with_context(|| format!("opening {}", node.display()))?;
        Ok(Self {
            device: gbm::Device::new(file).context("creating the GBM device")?,
        })
    }

    /// The single-plane layouts among `modifiers`, or all of them when there
    /// are none. A compressed layout's extra planes are lost on consumers that
    /// import one plane per format plane, GStreamer's among them.
    pub(super) fn prefer_single_plane(&self, fourcc: u32, modifiers: &[u64]) -> Vec<u64> {
        let Ok(format) = Format::try_from(fourcc) else {
            return modifiers.to_vec();
        };
        let single: Vec<u64> = modifiers
            .iter()
            .copied()
            .filter(|modifier| {
                self.device
                    .format_modifier_plane_count(format, Modifier::from(*modifier))
                    == Some(1)
            })
            .collect();
        if single.is_empty() {
            modifiers.to_vec()
        } else {
            single
        }
    }

    /// Allocates with the first of `modifiers` the driver can produce;
    /// [`IMPLICIT_MODIFIER`] alone leaves the layout to the driver.
    pub(super) fn allocate(
        &self,
        (width, height): (u32, u32),
        fourcc: u32,
        modifiers: &[u64],
    ) -> anyhow::Result<Allocation> {
        let format = Format::try_from(fourcc).map_err(|_| anyhow!("unknown fourcc {fourcc:#x}"))?;
        let usage = BufferObjectFlags::RENDERING;
        let implicit = modifiers
            .iter()
            .all(|modifier| *modifier == IMPLICIT_MODIFIER);
        let object: BufferObject<()> = if implicit {
            self.device
                .create_buffer_object(width, height, format, usage)?
        } else {
            let explicit = modifiers
                .iter()
                .filter(|modifier| **modifier != IMPLICIT_MODIFIER)
                .map(|modifier| Modifier::from(*modifier));
            self.device
                .create_buffer_object_with_modifiers2(width, height, format, explicit, usage)?
        };
        let planes: Vec<Plane> = (0..i32::try_from(object.plane_count())?)
            .map(|plane| {
                Ok(Plane {
                    fd: object.fd_for_plane(plane)?,
                    offset: object.offset(plane),
                    stride: object.stride_for_plane(plane),
                })
            })
            .collect::<anyhow::Result<_>>()?;
        let size = planes
            .first()
            .map(|plane| dmabuf_size(&plane.fd))
            .transpose()?
            .unwrap_or_default();
        let modifier = if implicit {
            IMPLICIT_MODIFIER
        } else {
            u64::from(object.modifier())
        };
        Ok(Allocation {
            planes,
            size,
            modifier,
            _object: object,
        })
    }
}

/// A dmabuf's size is where seeking to its end lands.
fn dmabuf_size(fd: &OwnedFd) -> anyhow::Result<u32> {
    let mut file = File::from(fd.try_clone()?);
    Ok(u32::try_from(file.seek(SeekFrom::End(0))?)?)
}

fn first_render_node() -> anyhow::Result<PathBuf> {
    std::fs::read_dir(DRI_DIRECTORY)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(RENDER_NODE_PREFIX))
        })
        .min()
        .context("no GPU render node")
}
