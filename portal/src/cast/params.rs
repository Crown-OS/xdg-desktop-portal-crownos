//! The SPA pods a stream negotiates with.

use std::io::Cursor;

use pipewire::spa::buffer::DataType;
use pipewire::spa::param::ParamType;
use pipewire::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pipewire::spa::pod::serialize::PodSerializer;
use pipewire::spa::pod::{self, ChoiceValue, Pod, Property, PropertyFlags, Value};
use pipewire::spa::sys::{
    SPA_META_Header, SPA_PARAM_BUFFERS_blocks, SPA_PARAM_BUFFERS_buffers,
    SPA_PARAM_BUFFERS_dataType, SPA_PARAM_META_size, SPA_PARAM_META_type, spa_meta_header,
};
use pipewire::spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Id, Rectangle, SpaTypes};

use super::formats::{FormatOffer, Offer};

/// The compositor's ring holds at most four buffers.
const MAX_BUFFERS: i32 = 4;
const MIN_BUFFERS: i32 = 2;
const DEFAULT_BUFFERS: i32 = 3;

/// Serialized pods, kept alive for as long as the borrowed [`Pod`]s are.
#[derive(Default)]
pub(super) struct Params(Vec<Vec<u8>>);

impl Params {
    pub(super) fn push(&mut self, object: pod::Object) -> anyhow::Result<()> {
        let (bytes, _) = PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))?;
        self.0.push(bytes.into_inner());
        Ok(())
    }

    pub(super) fn pods(&self) -> Vec<&Pod> {
        self.0
            .iter()
            .filter_map(|bytes| Pod::from_bytes(bytes))
            .collect()
    }
}

/// One `EnumFormat` per pixel format of `offer`, best first.
pub(super) fn enum_formats(offer: &Offer) -> anyhow::Result<Params> {
    let mut params = Params::default();
    for format in &offer.formats {
        params.push(video_format(format, &format.modifiers, offer))?;
    }
    Ok(params)
}

/// The format a consumer asked for with the modifier pinned to `modifier`,
/// followed by the full offer so the consumer can still fall back.
pub(super) fn fixated(
    format: &FormatOffer,
    modifier: u64,
    offer: &Offer,
) -> anyhow::Result<Params> {
    let mut params = Params::default();
    params.push(video_format(format, &[modifier], offer))?;
    for format in &offer.formats {
        params.push(video_format(format, &format.modifiers, offer))?;
    }
    Ok(params)
}

pub(super) fn buffers(plane_count: u32) -> anyhow::Result<Params> {
    let blocks = i32::try_from(plane_count)?;
    let dmabuf = 1 << DataType::DmaBuf.as_raw();
    let mut params = Params::default();
    params.push(pod::object!(
        SpaTypes::ObjectParamBuffers,
        ParamType::Buffers,
        Property::new(
            SPA_PARAM_BUFFERS_buffers,
            Value::Choice(ChoiceValue::Int(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Range {
                    default: DEFAULT_BUFFERS,
                    min: MIN_BUFFERS,
                    max: MAX_BUFFERS,
                },
            ))),
        ),
        Property::new(SPA_PARAM_BUFFERS_blocks, Value::Int(blocks)),
        Property::new(
            SPA_PARAM_BUFFERS_dataType,
            Value::Choice(ChoiceValue::Int(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Flags {
                    default: dmabuf,
                    flags: vec![dmabuf],
                },
            ))),
        ),
    ))?;
    params.push(pod::object!(
        SpaTypes::ObjectParamMeta,
        ParamType::Meta,
        Property::new(SPA_PARAM_META_type, Value::Id(Id(SPA_META_Header))),
        Property::new(
            SPA_PARAM_META_size,
            Value::Int(i32::try_from(size_of::<spa_meta_header>())?),
        ),
    ))?;
    Ok(params)
}

fn video_format(format: &FormatOffer, modifiers: &[u64], offer: &Offer) -> pod::Object {
    let alternatives: Vec<i64> = modifiers.iter().map(|modifier| *modifier as i64).collect();
    let dont_fixate = if alternatives.len() > 1 {
        PropertyFlags::DONT_FIXATE
    } else {
        PropertyFlags::empty()
    };
    let refresh = Fraction {
        num: offer.refresh_mhz,
        denom: 1000,
    };
    pod::object!(
        SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        pod::property!(FormatProperties::VideoFormat, Id, format.video),
        Property {
            key: FormatProperties::VideoModifier.as_raw(),
            flags: PropertyFlags::MANDATORY | dont_fixate,
            value: Value::Choice(ChoiceValue::Long(Choice(
                ChoiceFlags::empty(),
                ChoiceEnum::Enum {
                    default: alternatives.first().copied().unwrap_or_default(),
                    alternatives,
                },
            ))),
        },
        pod::property!(
            FormatProperties::VideoSize,
            Rectangle,
            Rectangle {
                width: offer.width,
                height: offer.height,
            }
        ),
        pod::property!(
            FormatProperties::VideoFramerate,
            Fraction,
            Fraction { num: 0, denom: 1 }
        ),
        pod::property!(
            FormatProperties::VideoMaxFramerate,
            Choice,
            Range,
            Fraction,
            refresh,
            Fraction { num: 1, denom: 1 },
            refresh
        ),
    )
}
