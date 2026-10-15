//! The pixel formats a cast can carry: what the compositor renders, narrowed to
//! what PipeWire video consumers read.

use pipewire::spa::param::video::VideoFormat;

/// A buffer allocated without an explicit modifier is described with it.
pub(super) const IMPLICIT_MODIFIER: u64 = 0x00ff_ffff_ffff_ffff;

/// Opaque formats first: a consumer that can take either is better off without
/// an alpha channel nobody reads.
const RGB_FORMATS: [(u32, VideoFormat); 4] = [
    (fourcc(*b"XR24"), VideoFormat::BGRx),
    (fourcc(*b"XB24"), VideoFormat::RGBx),
    (fourcc(*b"AR24"), VideoFormat::BGRA),
    (fourcc(*b"AB24"), VideoFormat::RGBA),
];

const fn fourcc(code: [u8; 4]) -> u32 {
    u32::from_le_bytes(code)
}

pub(super) fn drm_format(video: VideoFormat) -> Option<u32> {
    RGB_FORMATS
        .iter()
        .find_map(|(drm, known)| (*known == video).then_some(*drm))
}

/// One pixel format with the modifiers the compositor renders it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FormatOffer {
    pub(super) drm: u32,
    pub(super) video: VideoFormat,
    pub(super) modifiers: Vec<u64>,
}

/// Everything a stream advertises to its consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Offer {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) refresh_mhz: u32,
    pub(super) formats: Vec<FormatOffer>,
}

/// One constraints sequence of a compositor capture session, as it arrives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Constraints {
    pub(super) width: u32,
    pub(super) height: u32,
    formats: Vec<(u32, Vec<u64>)>,
}

impl Constraints {
    pub(super) fn begin(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.formats.clear();
    }

    pub(super) fn add_format(&mut self, format: u32) {
        if !self.formats.iter().any(|(known, _)| *known == format) {
            self.formats.push((format, Vec::new()));
        }
    }

    pub(super) fn add_modifier(&mut self, format: u32, modifier: u64) {
        if let Some((_, modifiers)) = self.formats.iter_mut().find(|(known, _)| *known == format) {
            modifiers.push(modifier);
        }
    }

    pub(super) fn accepts(&self, format: u32, modifier: u64) -> bool {
        self.formats
            .iter()
            .any(|(known, modifiers)| *known == format && modifiers.contains(&modifier))
    }

    pub(super) fn offer(&self, refresh_mhz: u32) -> Offer {
        let formats = RGB_FORMATS
            .iter()
            .filter_map(|(drm, video)| {
                let (_, modifiers) = self.formats.iter().find(|(known, _)| known == drm)?;
                (!modifiers.is_empty()).then(|| FormatOffer {
                    drm: *drm,
                    video: *video,
                    modifiers: modifiers.clone(),
                })
            })
            .collect();
        Offer {
            width: self.width,
            height: self.height,
            refresh_mhz,
            formats,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XRGB: u32 = fourcc(*b"XR24");
    const ARGB: u32 = fourcc(*b"AR24");
    const NV12: u32 = fourcc(*b"NV12");

    fn constraints(formats: &[(u32, &[u64])]) -> Constraints {
        let mut constraints = Constraints::default();
        constraints.begin(1920, 1080);
        for (format, modifiers) in formats {
            constraints.add_format(*format);
            for modifier in *modifiers {
                constraints.add_modifier(*format, *modifier);
            }
        }
        constraints
    }

    #[test]
    fn opaque_formats_are_offered_first() {
        let offer = constraints(&[(ARGB, &[0]), (XRGB, &[0, 1])]).offer(60_000);
        let order: Vec<VideoFormat> = offer.formats.iter().map(|format| format.video).collect();
        assert_eq!(order, [VideoFormat::BGRx, VideoFormat::BGRA]);
        assert_eq!(offer.formats[0].modifiers, [0, 1]);
    }

    #[test]
    fn formats_no_consumer_reads_are_left_out() {
        let offer = constraints(&[(NV12, &[0])]).offer(60_000);
        assert!(offer.formats.is_empty());
    }

    #[test]
    fn a_format_is_accepted_only_with_an_advertised_modifier() {
        let constraints = constraints(&[(XRGB, &[IMPLICIT_MODIFIER])]);
        assert!(constraints.accepts(XRGB, IMPLICIT_MODIFIER));
        assert!(!constraints.accepts(XRGB, 0));
        assert!(!constraints.accepts(ARGB, IMPLICIT_MODIFIER));
    }

    #[test]
    fn video_formats_map_back_to_their_fourcc() {
        assert_eq!(drm_format(VideoFormat::RGBA), Some(fourcc(*b"AB24")));
        assert_eq!(drm_format(VideoFormat::NV12), None);
    }
}
