use crate::gates::GateResult;

pub struct Submission<'a> {
    pub scope: &'a str,
    pub feedback: &'a str,
    pub reproduction: Option<&'a str>,
    pub backend: &'a str,
    pub model: Option<&'a str>,
    pub gates: &'a [(String, GateResult)],
    pub human_reviewed: bool,
}

/// Render the SPEC §3 metadata block embedded in every submission body.
pub fn render_block(s: &Submission) -> String {
    let mut out = String::from("<!-- auto-oss:v0\n");
    out.push_str(&format!("scope: {}\n", s.scope));
    push_block_scalar(&mut out, "feedback", s.feedback);
    if let Some(repro) = s.reproduction {
        push_block_scalar(&mut out, "reproduction", repro);
    }
    out.push_str("environment:\n");
    out.push_str(&format!(
        "  os: {} ({})\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str("agent:\n");
    out.push_str(&format!("  backend: {}\n", s.backend));
    if let Some(model) = s.model {
        out.push_str(&format!("  model: {model}\n"));
    }
    if !s.gates.is_empty() {
        out.push_str("gates:\n");
        for (name, result) in s.gates {
            out.push_str(&format!("  {name}: {result}\n"));
        }
    }
    out.push_str(&format!("human_reviewed: {}\n", s.human_reviewed));
    out.push_str(&format!("client: auto-oss/{}\n", env!("CARGO_PKG_VERSION")));
    out.push_str("-->");
    out
}

/// YAML block scalar, with the one sequence that would terminate the
/// surrounding HTML comment defused.
fn push_block_scalar(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{key}: |\n"));
    for line in value.replace("-->", "-- >").lines() {
        out.push_str(&format!("  {line}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::GateResult;

    #[test]
    fn renders_required_fields() {
        let gates = vec![("test".to_string(), GateResult::Pass)];
        let block = render_block(&Submission {
            scope: "bug-fix",
            feedback: "panics on empty input",
            reproduction: Some("run `foo` with no args"),
            backend: "claude-code",
            model: None,
            gates: &gates,
            human_reviewed: true,
        });
        assert!(block.starts_with("<!-- auto-oss:v0\n"));
        assert!(block.ends_with("-->"));
        assert!(block.contains("scope: bug-fix"));
        assert!(block.contains("  panics on empty input"));
        assert!(block.contains("  test: pass"));
        assert!(block.contains("human_reviewed: true"));
    }

    #[test]
    fn feedback_cannot_terminate_the_comment() {
        let block = render_block(&Submission {
            scope: "docs",
            feedback: "text with --> inside",
            reproduction: None,
            backend: "claude-code",
            model: None,
            gates: &[],
            human_reviewed: true,
        });
        assert_eq!(
            block.matches("-->").count(),
            1,
            "only the closing tag may appear"
        );
    }
}
