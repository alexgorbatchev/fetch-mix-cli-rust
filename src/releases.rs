use crate::{Error, Result, llm::Environment};

pub fn from_environment(
    prefix: &str,
    environment: &Environment,
) -> Result<Option<cli_deps::ReleaseChannel>> {
    let values = ["BASE_URL", "OWNER", "REPO"].map(|suffix| {
        environment
            .get(&format!("{prefix}_RUST_RELEASE_{suffix}"))
            .map(String::as_str)
            .unwrap_or_default()
            .trim()
    });
    if values.iter().all(|value| value.is_empty()) {
        return Ok(None);
    }
    if values.iter().any(|value| value.is_empty()) {
        return Err(Error::Invalid(format!(
            "{prefix}_RUST_RELEASE_BASE_URL, OWNER and REPO must all be configured"
        )));
    }
    let [base, owner, repository] = values;
    Ok(Some(cli_deps::ReleaseChannel::new(
        base, owner, repository,
    )?))
}
