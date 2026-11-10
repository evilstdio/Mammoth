//! Official source modules are embedded in the compiler and linked on import.
use crate::{jvm, syntax};
pub const REQUESTS: &str = include_str!("requests.mam");
pub fn emit<'a>(
    programs: &[&'a syntax::Program<'a>],
) -> Result<Vec<jvm::ClassFile>, (usize, syntax::Error)> {
    let needed = programs.iter().any(|program| {
        program.imports.iter().any(|import| {
            import.module == "mammoth"
                && matches!(
                    import.name.text,
                    "requests"
                        | "Response"
                        | "RequestError"
                        | "InvalidRequestError"
                        | "TimeoutError"
                        | "TransportError"
                        | "HttpStatusError"
                        | "Headers"
                        | "Session"
                        | "Auth"
                        | "Upload"
                        | "RedirectError"
                        | "JsonError"
                        | "Json"
                        | "JsonNull"
                        | "JsonBool"
                        | "JsonNumber"
                        | "JsonText"
                        | "JsonArray"
                        | "JsonObject"
                )
        })
    });
    if !needed {
        if programs.len() == 1 {
            return jvm::emit(programs[0]).map_err(|error| (0, error));
        }
        return jvm::emit_programs(programs);
    }
    let mut requests = syntax::parse(REQUESTS).map_err(|error| (programs.len(), error))?;
    // The bundled class facade occupies the reserved root import namespace.
    requests.module = Some("mammoth".into());
    requests.source_name = "mammoth.requests.mam".into();
    let mut all = programs.to_vec();
    all.push(&requests);
    jvm::emit_programs(&all)
}
