pub(crate) mod xml;
pub(crate) mod zip;

use crate::RenderError;

pub(crate) fn resolve_relationship(base: &str, target: &str) -> Result<String, RenderError> {
    let mut parts = base
        .rsplit_once('/')
        .map(|(parent, _)| parent.split('/').map(str::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();
    for part in target.replace('\\', "/").split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(RenderError::malformed(
                        "a relationship target escapes the document container; obtain a safe copy",
                    ));
                }
            }
            value => parts.push(value.to_owned()),
        }
    }
    Ok(parts.join("/"))
}
