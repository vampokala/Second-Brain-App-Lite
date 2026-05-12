//! Chat persona labels and system-prompt fragments.

use crate::config::AppConfig;
use serde::Serialize;

pub const ID_WIKI_MAINTAINER: &str = "wiki_maintainer";
pub const ID_SOFTWARE_ENGINEER: &str = "software_engineer";
pub const ID_BUSINESS_ANALYST: &str = "business_analyst";
pub const ID_PRODUCT_OWNER: &str = "product_owner";
pub const ID_TESTER: &str = "tester";
pub const ID_ARCHITECT: &str = "architect";
pub const ID_TECHNICAL_MANAGER: &str = "technical_manager";
pub const ID_STUDENT: &str = "student";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaMeta {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeOption {
    pub id: String,
    pub label: String,
}

pub fn all_personas() -> Vec<PersonaMeta> {
    vec![
        PersonaMeta {
            id: ID_WIKI_MAINTAINER.into(),
            label: "Wiki maintainer".into(),
        },
        PersonaMeta {
            id: ID_SOFTWARE_ENGINEER.into(),
            label: "Software engineer".into(),
        },
        PersonaMeta {
            id: ID_BUSINESS_ANALYST.into(),
            label: "Business analyst".into(),
        },
        PersonaMeta {
            id: ID_PRODUCT_OWNER.into(),
            label: "Product owner".into(),
        },
        PersonaMeta {
            id: ID_TESTER.into(),
            label: "Tester / QA".into(),
        },
        PersonaMeta {
            id: ID_ARCHITECT.into(),
            label: "Architect".into(),
        },
        PersonaMeta {
            id: ID_TECHNICAL_MANAGER.into(),
            label: "Technical manager".into(),
        },
        PersonaMeta {
            id: ID_STUDENT.into(),
            label: "Student".into(),
        },
    ]
}

pub fn student_grade_options() -> Vec<GradeOption> {
    let mut v = vec![GradeOption {
        id: "K".into(),
        label: "Kindergarten".into(),
    }];
    for n in 1..=12 {
        v.push(GradeOption {
            id: n.to_string(),
            label: format!("Grade {n}"),
        });
    }
    v
}

/// Canonical persona id for prompts and UI.
pub fn normalize_chat_persona(raw: &str) -> &'static str {
    match raw.trim() {
        ID_SOFTWARE_ENGINEER => ID_SOFTWARE_ENGINEER,
        ID_BUSINESS_ANALYST => ID_BUSINESS_ANALYST,
        ID_PRODUCT_OWNER => ID_PRODUCT_OWNER,
        ID_TESTER => ID_TESTER,
        ID_ARCHITECT => ID_ARCHITECT,
        ID_TECHNICAL_MANAGER => ID_TECHNICAL_MANAGER,
        ID_STUDENT => ID_STUDENT,
        ID_WIKI_MAINTAINER | "" => ID_WIKI_MAINTAINER,
        _ => ID_WIKI_MAINTAINER,
    }
}

/// Valid grade token for prompts; invalid → `"9"`.
pub fn resolved_student_grade_token(grade: &str) -> &'static str {
    let g = grade.trim();
    if g.eq_ignore_ascii_case("k") {
        return "K";
    }
    if let Ok(n) = g.parse::<u32>() {
        if (1..=12).contains(&n) {
            return match n {
                1 => "1",
                2 => "2",
                3 => "3",
                4 => "4",
                5 => "5",
                6 => "6",
                7 => "7",
                8 => "8",
                9 => "9",
                10 => "10",
                11 => "11",
                12 => "12",
                _ => "9",
            };
        }
    }
    "9"
}

pub fn grade_display_label(grade_token: &str) -> String {
    match grade_token.trim() {
        "K" | "k" => "Kindergarten".into(),
        n => {
            if let Ok(i) = n.parse::<u32>() {
                if (1..=12).contains(&i) {
                    return format!("Grade {i}");
                }
            }
            "Grade 9".into()
        }
    }
}

pub fn persona_label_for_id(id: &str) -> &'static str {
    match normalize_chat_persona(id) {
        ID_WIKI_MAINTAINER => "Wiki maintainer",
        ID_SOFTWARE_ENGINEER => "Software engineer",
        ID_BUSINESS_ANALYST => "Business analyst",
        ID_PRODUCT_OWNER => "Product owner",
        ID_TESTER => "Tester / QA",
        ID_ARCHITECT => "Architect",
        ID_TECHNICAL_MANAGER => "Technical manager",
        ID_STUDENT => "Student",
        _ => "Wiki maintainer",
    }
}

/// Short string for UI chip and retrieval meta (e.g. `Student · Grade 7`).
pub fn persona_display(cfg: &AppConfig) -> String {
    let id = normalize_chat_persona(&cfg.chat_persona);
    if id == ID_STUDENT {
        let tok = resolved_student_grade_token(&cfg.student_grade);
        format!("Student · {}", grade_display_label(tok))
    } else {
        persona_label_for_id(id).to_string()
    }
}

pub fn persona_addon_applied(cfg: &AppConfig) -> bool {
    !cfg.persona_prompt_addon.trim().is_empty()
}

fn fragment_for_id(id: &str, student_grade_token: Option<&str>) -> String {
    match id {
        ID_WIKI_MAINTAINER => "You are using the default **wiki maintainer** voice: prioritize accuracy, vault-appropriate terminology, and alignment with CLAUDE.md and llm-wiki.md. Keep answers well-structured for a personal knowledge base.".into(),
        ID_SOFTWARE_ENGINEER => "You are assisting as a **software engineer**: prefer concrete implementation guidance, debugging steps, tradeoffs, and small verifiable suggestions. When the evidence is thin, say so; do not invent repo-specific details.".into(),
        ID_BUSINESS_ANALYST => "You are assisting as a **business analyst**: clarify requirements, acceptance criteria, assumptions, and open questions. Use precise, stakeholder-friendly language and separate facts (from excerpts) from interpretation.".into(),
        ID_PRODUCT_OWNER => "You are assisting as a **product owner**: emphasize outcomes, user value, prioritization, risks, and crisp acceptance-style framing. Keep scope explicit and flag ambiguities.".into(),
        ID_TESTER => "You are assisting as a **tester / QA**: think in terms of scenarios, edge cases, negative paths, repro steps, and testability. Call out missing information needed to validate behavior.".into(),
        ID_ARCHITECT => "You are assisting as a **software architect**: emphasize boundaries, interfaces, non-functional requirements, consistency, and evolution of the system. Prefer diagrams-in-words when helpful.".into(),
        ID_TECHNICAL_MANAGER => "You are assisting as a **technical manager**: balance delivery, risk, dependencies, and communication. Summarize decisions, owners, and next steps when appropriate.".into(),
        ID_STUDENT => {
            let tok = student_grade_token.unwrap_or("9");
            let level = grade_display_label(tok);
            format!(
                "You are assisting a **student** at **{level}**. Use age-appropriate vocabulary and clear step-by-step explanations. Encourage understanding over jargon; define terms when needed. Do not assume university or professional workplace context unless the provided material clearly requires it."
            )
        }
        _ => fragment_for_id(ID_WIKI_MAINTAINER, None),
    }
}

/// Inner persona instructions (without markdown headings for add-on).
pub fn persona_prompt_fragment(cfg: &AppConfig) -> String {
    let id = normalize_chat_persona(&cfg.chat_persona);
    let grade = if id == ID_STUDENT {
        Some(resolved_student_grade_token(&cfg.student_grade))
    } else {
        None
    };
    fragment_for_id(id, grade)
}

/// Full block: `### User persona` + base + optional `### User-provided persona notes`.
pub fn build_persona_system_section(cfg: &AppConfig) -> String {
    let base = persona_prompt_fragment(cfg);
    let mut out = format!("### User persona\n{base}\n");
    let addon = cfg.persona_prompt_addon.trim();
    if !addon.is_empty() {
        out.push_str("\n### User-provided persona notes\n");
        out.push_str(addon);
        out.push('\n');
    }
    out.push('\n');
    out
}
