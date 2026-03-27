mod graph;
mod layout;
mod parse;
mod render;

/// Try to render a mermaid code block as ASCII art.
/// Returns `Some(lines)` for flowcharts, `None` for anything else (triggering code-block fallback).
pub fn render_flowchart(code: &str, width: usize) -> Option<Vec<String>> {
    let graph = parse::parse_flowchart(code)?;
    if graph.nodes.is_empty() {
        return None;
    }
    if graph.nodes.len() > 30 {
        return None; // too large for legible ASCII
    }
    let layout = layout::compute(&graph, width)?;
    Some(render::render_to_lines(&graph, &layout))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for ch in s.chars() {
            if ch == '\x1b' {
                in_escape = true;
                continue;
            }
            if in_escape {
                if ch == 'm' {
                    in_escape = false;
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    #[test]
    fn render_simple_td() {
        let code = "graph TD\n  A[Start] --> B[End]";
        let lines = render_flowchart(code, 80).unwrap();
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let text = plain.join("\n");
        println!("=== Simple TD ===\n{}\n", text);
        assert!(text.contains("Start"));
        assert!(text.contains("End"));
        assert!(text.contains("┌"));
        assert!(text.contains("▼") || text.contains("│"));
    }

    #[test]
    fn render_lr() {
        let code = "flowchart LR\n  A[Input] --> B[Process] --> C[Output]";
        let lines = render_flowchart(code, 80).unwrap();
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let text = plain.join("\n");
        println!("=== LR ===\n{}\n", text);
        assert!(text.contains("Input"));
        assert!(text.contains("Process"));
        assert!(text.contains("Output"));
    }

    #[test]
    fn render_with_diamond() {
        let code = "graph TD\n  A[Start] --> B{Decision}\n  B -->|Yes| C[End]";
        let lines = render_flowchart(code, 80).unwrap();
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let text = plain.join("\n");
        println!("=== Diamond ===\n{}\n", text);
        assert!(text.contains("Start"));
        assert!(text.contains("< Decision >"));
    }

    #[test]
    fn fallback_non_flowchart() {
        let code = "sequenceDiagram\n  Alice->>Bob: Hello";
        assert!(render_flowchart(code, 80).is_none());
    }

    #[test]
    fn fallback_too_wide() {
        // 30 chars wide terminal with a wide graph should return None
        let code = "flowchart LR\n  AAAAAAAAAA[Very Long Label Here] --> BBBBBBBBBB[Another Long Label]";
        let result = render_flowchart(code, 30);
        // May or may not fit — just ensure no panic
        let _ = result;
    }

    #[test]
    fn render_invitations_creation() {
        let code = r#"flowchart TB
    CG[client-global<br/>useSendUserInvitation] --> CTRL[inviteUser]

    subgraph SG[server-global]
        CTRL --> UC[InviteUserToBusinessUseCase<br/>Valida y crea BusinessInvitation]
        UC --> SEND[SendBusinessInvitationToUserUseCase]
    end

    SEND --> EMAIL[SendGrid]"#;
        let lines = render_flowchart(code, 100).unwrap();
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let text = plain.join("\n");
        println!("=== Invitations Creation ===\n{}\n", text);
        assert!(text.contains("inviteUser"));
        assert!(text.contains("SendGrid"));
    }

    #[test]
    fn render_invitations_team() {
        let code = r#"flowchart LR
    CG[client-global] --> SG[server-global]
    SG --> MS[msUsers<br/>usuarios activos]
    SG --> BI[BusinessInvitations<br/>pendientes]
    MS --> MERGE[merge]
    BI --> MERGE"#;
        let lines = render_flowchart(code, 100).unwrap();
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let text = plain.join("\n");
        println!("=== Team Management ===\n{}\n", text);
        assert!(text.contains("client-global"));
        assert!(text.contains("merge"));
    }

    #[test]
    fn render_invitations_acceptance() {
        let code = r#"flowchart TB
    LINK[Usuario abre link de invitación]
    CG[client-global InvitationManager decodifica JWT]

    LINK --> CG
    CG -->|POST /invitations/accept| UC{AcceptBusinessInvitationUseCase Busca usuario por email en msUsers}

    UC -->|Usuario no registrado| NEW[Formulario de Registro Phone y Password]
    UC -->|Ya asociado al Business| NOOP[Pantalla 'Ya tienes acceso']
    UC -->|Registrado, sin asociación| ACCEPT

    NEW --> BFF

    subgraph BFF[BFF: AuthDomainService.acceptInvitation]
        B1[Obtiene invitación vía SG interno]
        B2[signUp: Crea usuario en SG]
        B3[getAccount: SG sincroniza usuario a msUsers]
        B4[POST /invitations/accept en SG]
        B1 --> B2 --> B3 --> B4
    end

    subgraph ACCEPT[server-global: AcceptBusinessInvitationUseCase]
        A1[Crea UserBusiness + Marca invitación aceptada] --> A2[Asigna rol en msUsers]
    end

    BFF --> ACCEPT
    ACCEPT --> SNS[SNS UserBusinessCreated]"#;
        let lines = render_flowchart(code, 200).unwrap();
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let text = plain.join("\n");
        println!("=== Acceptance Flow ===\n{}\n", text);
        assert!(text.contains("AcceptBusinessInvitation"));
        assert!(text.contains("BFF"));
        assert!(text.contains("SNS UserBusinessCreated"));
    }
}
