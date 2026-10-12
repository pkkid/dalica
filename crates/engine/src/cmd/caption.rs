//! Captions: `photo.caption.set` (`organizing.md` §5.3). The inline caption editors (Picture ▸ Edit
//! Caption in the library, the caption field of the edit view) commit through it.
//!
//! The caption is trimmed and cut to [`dalica_library::MAX_CAPTION`] bytes at a character boundary
//! ([`dalica_library::clean_caption`]); what is left empty clears it. Every photo named gets the
//! same caption in one library undo step, and none when no caption changes.

use dalica_core::PhotoId;
use dalica_library::{Op, clean_caption};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "photo.caption.set",
        label: "Set Caption",
        menu: "",
        keys: &[],
        params: r#"{"ids":[PhotoId], "caption":string}: set the photos' caption (trimmed, at most 16 KiB; "" clears it)"#,
        effect: Effect::Library,
        history: History::Library,
        enabled: has_ids,
        run: set,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetCaption {
    ids: Vec<PhotoId>,
    caption: String,
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SetCaption { ids, caption } = params(p)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let caption = clean_caption(&caption);
    let changed: Vec<(PhotoId, String)> = ids.iter().filter(|id| s.library().caption(**id) != caption).map(|id| (*id, caption.clone())).collect();
    let n = changed.len();
    if n > 0 {
        s.apply(Op::SetCaptions { captions: changed })?;
    }
    Ok(json!({ "ids": ids, "caption": caption, "changed": n }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{hostile, library};
    use crate::error::CmdError;

    #[test]
    fn disabled_and_hostile_params() {
        let (_dirs, mut s, ids) = library(1);
        assert_eq!(
            s.execute("photo.caption.set", &json!({ "ids": [], "caption": "x" })),
            Err(CmdError::Disabled("Select some photos first.".into()))
        );
        assert!(matches!(s.execute("photo.caption.set", &json!({ "ids": [999], "caption": "x" })), Err(CmdError::NotFound(_))));
        for bad in [json!({ "ids": [ids[0]] }), json!({ "ids": [ids[0]], "caption": 3 }), json!({ "ids": [ids[0]], "caption": null })] {
            assert!(matches!(s.execute("photo.caption.set", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        hostile(&mut s, "photo.caption.set");
        assert_eq!(s.undo_stack().len(), 0);
    }
}
