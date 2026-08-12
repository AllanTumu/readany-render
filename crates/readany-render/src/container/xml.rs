use crate::{Limits, RenderError};
use quick_xml::events::Event;

pub(crate) fn validate(bytes: &[u8], limits: &Limits) -> Result<(), RenderError> {
    if bytes
        .windows(9)
        .any(|window| window.eq_ignore_ascii_case(b"<!DOCTYPE"))
        || bytes
            .windows(8)
            .any(|window| window.eq_ignore_ascii_case(b"<!ENTITY"))
    {
        return Err(RenderError::malformed(
            "the XML declares entities, which are disabled; obtain a document without external or expanded entities",
        ));
    }
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().check_end_names = true;
    let mut depth = 0_u32;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| RenderError::limit("xml_depth", u64::MAX))?;
                if depth > limits.xml_depth {
                    return Err(RenderError::limit("xml_depth", u64::from(depth)));
                }
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Eof) => return Ok(()),
            Ok(_) => {}
            Err(_) => {
                return Err(RenderError::malformed(
                    "an XML document part is malformed; obtain a fresh copy",
                ));
            }
        }
    }
}

pub(crate) fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}
