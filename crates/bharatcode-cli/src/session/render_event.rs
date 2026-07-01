use super::status_line;
use super::terminal_width;

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum SessionRenderEvent {
    AssistantText {
        text: String,
    },
    ToolCall {
        name: String,
        summary: Option<String>,
    },
    TaskNotification {
        body: String,
    },
    StatusFooter {
        model: String,
        provider: String,
        context_pct: u8,
        rupees_spent: Option<f64>,
    },
    ApprovalRequest {
        title: String,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub width: usize,
}

impl RenderOptions {
    pub fn new(width: usize) -> Self {
        Self { width }
    }
}

pub fn render_event(event: &SessionRenderEvent, options: RenderOptions) -> Vec<String> {
    let width = options.width;
    match event {
        SessionRenderEvent::AssistantText { text } => width_safe_lines(text, width),
        SessionRenderEvent::ToolCall { name, summary } => {
            let line = match summary {
                Some(summary) if !summary.trim().is_empty() => {
                    format!("tool: {name} - {}", summary.trim())
                }
                _ => format!("tool: {name}"),
            };
            vec![truncate_line(&line, width)]
        }
        SessionRenderEvent::TaskNotification { body } => indented_lines(body, "    ", width),
        SessionRenderEvent::StatusFooter {
            model,
            provider,
            context_pct,
            rupees_spent,
        } => {
            let ctx = status_line::StatusCtx {
                model,
                provider,
                context_pct: *context_pct,
                rupees_spent: *rupees_spent,
                width_budget: width,
            };
            vec![truncate_line(&status_line::format_status(ctx), width)]
        }
        SessionRenderEvent::ApprovalRequest { title, detail } => {
            let mut lines = vec![truncate_line(&format!("approval: {title}"), width)];
            if let Some(detail) = detail {
                lines.extend(indented_lines(detail, "  ", width));
            }
            lines
        }
    }
}

fn width_safe_lines(text: &str, width: usize) -> Vec<String> {
    let lines: Vec<String> = text
        .lines()
        .map(|line| truncate_line(line, width))
        .collect();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn indented_lines(text: &str, prefix: &str, width: usize) -> Vec<String> {
    text.lines()
        .map(|line| {
            let body_budget = width.saturating_sub(terminal_width::display_width(prefix));
            format!("{prefix}{}", truncate_line(line, body_budget))
        })
        .collect()
}

fn truncate_line(line: &str, width: usize) -> String {
    terminal_width::truncate_to_width(line, width, "...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_no_color<T>(f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("NO_COLOR");
        std::env::set_var("NO_COLOR", "1");
        let out = f();
        match previous {
            Some(value) => std::env::set_var("NO_COLOR", value),
            None => std::env::remove_var("NO_COLOR"),
        }
        out
    }

    fn assert_width(lines: &[String], width: usize) {
        for line in lines {
            assert!(
                terminal_width::display_width(line) <= width,
                "line exceeded width {width}: {line:?}"
            );
        }
    }

    #[test]
    fn task_notifications_are_indented_and_width_safe() {
        let event = SessionRenderEvent::TaskNotification {
            body: "first line\nsecond line with a lot of trailing detail".to_string(),
        };
        let lines = render_event(&event, RenderOptions::new(18));

        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.starts_with("    ")));
        assert_width(&lines, 18);
    }

    #[test]
    fn status_footer_uses_shared_formatter_and_width_budget() {
        let lines = with_no_color(|| {
            render_event(
                &SessionRenderEvent::StatusFooter {
                    model: "very-long-model-name".to_string(),
                    provider: "test-provider".to_string(),
                    context_pct: 42,
                    rupees_spent: Some(12.0),
                },
                RenderOptions::new(32),
            )
        });

        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("model"));
        assert_width(&lines, 32);
    }

    #[test]
    fn tool_and_approval_events_are_deterministic() {
        let tool = render_event(
            &SessionRenderEvent::ToolCall {
                name: "developer__shell".to_string(),
                summary: Some("cargo test".to_string()),
            },
            RenderOptions::new(80),
        );
        let approval = render_event(
            &SessionRenderEvent::ApprovalRequest {
                title: "run command".to_string(),
                detail: Some("cargo test -p bharatcode-cli".to_string()),
            },
            RenderOptions::new(80),
        );

        assert_eq!(tool, vec!["tool: developer__shell - cargo test"]);
        assert_eq!(
            approval,
            vec![
                "approval: run command".to_string(),
                "  cargo test -p bharatcode-cli".to_string()
            ]
        );
    }
}
