# Chat personas

Second Brain Lite can shape **how** the model answers (tone, structure, and role-specific priorities) while **wiki** and **web** toggles still control **what evidence** is retrieved for each message.

## Where to configure

Open **Settings** → **Chat** (or use the persona link next to the checkboxes on the **Chat** screen).

- **Persona** — Built-in role (see table below). Applies to every chat until you change it.
- **Grade** — Shown only when persona is **Student** (Kindergarten through Grade 12). Stored in your app config so switching away from Student and back restores your last grade.
- **Additional persona instructions** — Optional free text appended **after** the built-in persona text on **every** send (for any persona). Use it for stack, team norms, depth, or examples you want the model to favor.

Click **Save chat settings** to persist changes to `config.json` in app data.

## Built-in personas

| Id | Label |
|----|--------|
| `wiki_maintainer` | Wiki maintainer (default) |
| `software_engineer` | Software engineer |
| `business_analyst` | Business analyst |
| `product_owner` | Product owner |
| `tester` | Tester / QA |
| `architect` | Architect |
| `technical_manager` | Technical manager |
| `small_business_owner` | Small business owner |
| `student` | Student |

Default persona is **Wiki maintainer**. Default student grade token is **9** (Grade 9) if the stored value is missing or invalid.

## Chat screen

Next to **Wiki sources only** and **Web search**, the app shows the **current persona** (and grade when Student). That label links to **Settings** so you can change persona quickly.

After each completed send, the **Last send** line in the footer includes the persona used for **that** message and whether **custom notes** (additional instructions) were on.

## Grounding rules unchanged

Personas do **not** turn on wiki retrieval or web search by themselves. Those remain the checkboxes above the composer. When wiki-only or web is off, the same caveats as before apply; the model must not claim vault or live-web evidence it did not receive for that message.

## Config keys (app data)

Stored in `config.json` (camelCase in JSON):

| Key | Meaning |
|-----|--------|
| `chatPersona` | Persona id from the table above |
| `studentGrade` | `K` or `1`–`12` when using Student |
| `personaPromptAddon` | Optional string; empty means no add-on block |

## Maintainer note

Built-in persona wording lives in Rust: [`src-tauri/src/personas.rs`](../src-tauri/src/personas.rs). The UI label list is also exposed via Tauri commands `list_chat_personas` and `list_student_grade_options` so Settings stays aligned with the backend.
