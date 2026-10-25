//! Small helpers shared by the editor panels for turning UI actions into engine commands.

use furnicraft_engine::{CommandOutcome, Session};
use serde::Serialize;
use serde_json::Value;

pub(crate) fn json<T: Serialize>(params: &T) -> Result<Value, String> {
    serde_json::to_value(params).map_err(|error| format!("could not encode command parameters: {error}"))
}

pub(crate) fn dispatch(session: &mut Session, id: &str, params: &Value) -> Result<CommandOutcome, String> {
    session.dispatch(id, params).map_err(|error| error.to_string())
}
