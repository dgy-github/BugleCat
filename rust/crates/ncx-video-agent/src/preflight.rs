use crate::Result;

pub fn resolve_paid_seedance_prereqs<Tos, Ark, LoadTos, LoadArk>(
    load_tos: LoadTos,
    load_ark: LoadArk,
) -> Result<(Tos, Ark)>
where
    LoadTos: FnOnce() -> Result<Tos>,
    LoadArk: FnOnce() -> Result<Ark>,
{
    let tos = load_tos()?;
    let ark = load_ark()?;
    Ok((tos, ark))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::VideoAgentError;

    #[test]
    fn paid_seedance_prereqs_do_not_resolve_ark_when_tos_is_missing() {
        let ark_called = Cell::new(false);
        let err = resolve_paid_seedance_prereqs(
            || Err::<&str, _>(VideoAgentError::Tos("missing tos".to_string())),
            || {
                ark_called.set(true);
                Ok("ark-key")
            },
        )
        .expect_err("missing TOS should stop paid preflight");

        assert!(!ark_called.get());
        assert!(matches!(err, VideoAgentError::Tos(_)));
    }

    #[test]
    fn paid_seedance_prereqs_resolve_tos_before_ark() {
        let calls = RefCell::new(Vec::new());
        let (tos, ark) = resolve_paid_seedance_prereqs(
            || {
                calls.borrow_mut().push("tos");
                Ok("tos-config")
            },
            || {
                calls.borrow_mut().push("ark");
                Ok("ark-key")
            },
        )
        .unwrap();

        assert_eq!(tos, "tos-config");
        assert_eq!(ark, "ark-key");
        assert_eq!(calls.into_inner(), vec!["tos", "ark"]);
    }
}
