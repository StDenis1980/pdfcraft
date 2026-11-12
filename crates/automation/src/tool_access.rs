//! The tools automation may run: `pdfcraft-cli mcp --allow-commands LIST`.
//!
//! The same flag as Texto, Calculo and Presenta, for PdfCraft's tool-based server. A list names
//! tools (`page_rotate`), tool groups (`page_*`: every tool whose name starts with `page_`),
//! registry command ids (`page.rotate`: the tool that automates it) and command groups
//! (`page.*`: the tools of every command whose id starts with `page.`), separated by commas,
//! spaces or new lines; `#` starts a comment. An unknown name, a group that matches nothing and an
//! empty list are errors, so a typo never leaves the agent with less, or more, than meant.
//!
//! While a list is in force, every tool a client asks for must be on it: a `tools/call`, the tool
//! behind `tool_call` (compact mode), and the tool behind each `command_run` and `command_batch`
//! step (which also need `command_run` / `command_batch` themselves). A tool a tool runs itself is
//! part of the tool that was allowed and isn't checked again. `tools/list` and `tool_search` show
//! only the allowed tools. Resources (`pdfcraft://…`) read documents without a tool call and stay
//! available.

use std::collections::BTreeSet;

use crate::tools;

/// Longest list accepted (bytes).
pub const MAX_LIST_BYTES: usize = 256 * 1024;

/// The tools automation may run, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolAllowlist {
    tools: BTreeSet<&'static str>,
    /// What the list said, for logs.
    asked: Vec<String>,
}

impl ToolAllowlist {
    /// Parse `list` (see the module documentation).
    pub fn parse(list: &str) -> Result<Self, String> {
        if list.len() > MAX_LIST_BYTES {
            return Err(format!("command list too long: {} bytes (at most {MAX_LIST_BYTES})", list.len()));
        }
        let all = tools::tools();
        let mut out = Self::default();
        for line in list.lines() {
            let line = line.split_once('#').map_or(line, |(before, _)| before);
            for item in line.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()) {
                let matched: Vec<&'static str> = if let Some(prefix) = item.strip_suffix(".*") {
                    if prefix.is_empty() || prefix.contains('*') {
                        return Err(format!("bad command group `{item}`: use `group.*` or `tool_*`"));
                    }
                    let dotted = format!("{prefix}.");
                    all.iter().filter(|t| t.command.is_some_and(|c| c.starts_with(&dotted))).map(|t| t.name).collect()
                } else if let Some(prefix) = item.strip_suffix('*') {
                    if prefix.is_empty() || prefix.contains('*') || !prefix.ends_with('_') {
                        return Err(format!("bad tool group `{item}`: use `tool_*` or `group.*`"));
                    }
                    all.iter().filter(|t| t.name.starts_with(prefix)).map(|t| t.name).collect()
                } else if item.contains('*') {
                    return Err(format!("bad pattern `{item}`: only a trailing `_*` or `.*` is allowed"));
                } else if let Some(t) = all.iter().find(|t| t.name == item) {
                    vec![t.name]
                } else if let Some(tool) = tools::tool_for_command(item) {
                    vec![tool]
                } else if pdfcraft_engine::commands::command(item).is_some() {
                    return Err(format!("command `{item}` has no headless tool, so it can't be allowed (pdfcraft-cli tools lists them)"));
                } else {
                    return Err(format!("unknown tool or command `{item}` (pdfcraft-cli tools lists them)"));
                };
                if matched.is_empty() {
                    return Err(format!("`{item}` matches no tool"));
                }
                out.tools.extend(matched);
                out.asked.push(item.to_string());
            }
        }
        if out.tools.is_empty() {
            return Err("the command list is empty".into());
        }
        out.asked.sort();
        out.asked.dedup();
        Ok(out)
    }

    /// Whether the tool `name` may run.
    pub fn allows(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    /// What the list named, sorted, for logs.
    pub fn describe(&self) -> &[String] {
        &self.asked
    }

    /// Every allowed tool, sorted.
    pub fn tools(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tools.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_groups_and_command_ids_select_tools() {
        let list = ToolAllowlist::parse("doc_open, text_* # reading\npage.rotate\n").unwrap();
        assert!(list.allows("doc_open"));
        assert!(list.allows("text_extract") && list.allows("text_find"));
        assert!(list.allows("page_rotate"), "a command id allows the tool that automates it");
        assert!(!list.allows("doc_save") && !list.allows("command_run"));
        assert_eq!(list.describe(), ["doc_open", "page.rotate", "text_*"]);
    }

    #[test]
    fn typos_empty_lists_and_bad_patterns_are_refused() {
        for (list, needle) in [
            ("doc_opn", "unknown"),
            ("nope_*", "matches no tool"),
            ("nope.*", "matches no tool"),
            ("*", "bad"),
            ("doc*", "bad tool group"),
            ("d*c_open", "bad pattern"),
            ("", "empty"),
            (" , # only a comment", "empty"),
        ] {
            let e = ToolAllowlist::parse(list).unwrap_err();
            assert!(e.contains(needle), "{list:?}: {e}");
        }
        let long = format!("doc_open\n{}", "#".repeat(MAX_LIST_BYTES));
        assert!(ToolAllowlist::parse(&long).unwrap_err().contains("too long"));
    }
}
