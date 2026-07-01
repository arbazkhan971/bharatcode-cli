use bharatcode_core::agents::subagent_execution_tool::lib::TaskStatus;
use bharatcode_core::agents::subagent_execution_tool::notification_events::{
    TaskExecutionNotificationEvent, TaskInfo,
};
use bharatcode_core::utils::safe_truncate;
use console::Term;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

use super::terminal_width;

#[cfg(test)]
mod tests;

pub const TASK_EXECUTION_NOTIFICATION_TYPE: &str = "task_execution";

static INITIAL_SHOWN: AtomicBool = AtomicBool::new(false);
const TASK_DISPLAY_WIDTH_ENV: &str = "BHARATCODE_TASK_DISPLAY_WIDTH";
const DEFAULT_TASK_DISPLAY_WIDTH: usize = 100;
const MIN_TASK_DISPLAY_WIDTH: usize = 32;

#[derive(Clone, Copy)]
struct TaskDisplayOptions {
    plain: bool,
    width: usize,
}

impl TaskDisplayOptions {
    fn from_env() -> Self {
        let terminal_width = Term::stdout()
            .size_checked()
            .map(|(_, cols)| cols as usize)
            .unwrap_or(DEFAULT_TASK_DISPLAY_WIDTH);
        let width = std::env::var(TASK_DISPLAY_WIDTH_ENV)
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|width| *width > 0)
            .unwrap_or(terminal_width)
            .max(MIN_TASK_DISPLAY_WIDTH);

        Self {
            plain: plain_mode(),
            width,
        }
    }
}

fn plain_mode() -> bool {
    std::env::var_os("NO_COLOR").is_some()
        || env_truthy("BHARATCODE_A11Y")
        || env_truthy("BHARATCODE_SCREEN_READER")
}

fn env_truthy(key: &str) -> bool {
    std::env::var(key)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn sanitize_inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    terminal_width::truncate_to_width(text, max_width, "...")
}

fn push_display_line(out: &mut String, options: TaskDisplayOptions, line: impl AsRef<str>) {
    out.push_str(&truncate_to_width(line.as_ref(), options.width));
    out.push('\n');
}

fn format_result_data_for_display(result_data: &Value) -> String {
    match result_data {
        Value::String(s) => s.to_string(),
        Value::Object(obj) => {
            if let Some(partial_output) = obj.get("partial_output").and_then(|v| v.as_str()) {
                format!("Partial output: {}", partial_output)
            } else {
                serde_json::to_string_pretty(obj).unwrap_or_default()
            }
        }
        Value::Array(arr) => serde_json::to_string_pretty(arr).unwrap_or_default(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => "null".to_string(),
    }
}

fn process_output_for_display(output: &str) -> String {
    const MAX_OUTPUT_LINES: usize = 2;
    const OUTPUT_PREVIEW_LENGTH: usize = 100;

    let lines: Vec<&str> = output.lines().collect();
    let recent_lines = if lines.len() > MAX_OUTPUT_LINES {
        &lines[lines.len() - MAX_OUTPUT_LINES..]
    } else {
        &lines
    };

    let clean_output = recent_lines.join(" ... ");
    safe_truncate(&clean_output, OUTPUT_PREVIEW_LENGTH)
}

pub fn format_task_execution_notification(
    data: &Value,
) -> Option<(String, Option<String>, Option<String>)> {
    if let Ok(event) = serde_json::from_value::<TaskExecutionNotificationEvent>(data.clone()) {
        return Some(match event {
            TaskExecutionNotificationEvent::LineOutput { output, .. } => (
                format!("{}\n", output),
                None,
                Some(TASK_EXECUTION_NOTIFICATION_TYPE.to_string()),
            ),
            TaskExecutionNotificationEvent::TasksUpdate { .. } => {
                let formatted_display = format_tasks_update_from_event(&event);
                (
                    formatted_display,
                    None,
                    Some(TASK_EXECUTION_NOTIFICATION_TYPE.to_string()),
                )
            }
            TaskExecutionNotificationEvent::TasksComplete { .. } => {
                let formatted_summary = format_tasks_complete_from_event(&event);
                (
                    formatted_summary,
                    None,
                    Some(TASK_EXECUTION_NOTIFICATION_TYPE.to_string()),
                )
            }
        });
    }
    None
}

fn format_tasks_update_from_event(event: &TaskExecutionNotificationEvent) -> String {
    format_tasks_update_from_event_with_options(event, TaskDisplayOptions::from_env())
}

fn format_tasks_update_from_event_with_options(
    event: &TaskExecutionNotificationEvent,
    options: TaskDisplayOptions,
) -> String {
    if let TaskExecutionNotificationEvent::TasksUpdate { stats, tasks } = event {
        let mut display = String::new();

        if !INITIAL_SHOWN.swap(true, Ordering::SeqCst) {
            let heading = if options.plain {
                "Task Execution Dashboard"
            } else {
                "🎯 Task Execution Dashboard"
            };
            push_display_line(&mut display, options, heading);
            if options.plain {
                push_display_line(&mut display, options, "------------------------");
            } else {
                push_display_line(&mut display, options, "═══════════════════════════");
            }
            display.push('\n');
        }

        let progress = if options.plain {
            format!(
                "Progress: {} total | {} pending | {} running | {} completed | {} failed",
                stats.total, stats.pending, stats.running, stats.completed, stats.failed
            )
        } else {
            format!(
                "📊 Progress: {} total | ⏳ {} pending | 🏃 {} running | ✅ {} completed | ❌ {} failed",
                stats.total, stats.pending, stats.running, stats.completed, stats.failed
            )
        };
        push_display_line(&mut display, options, progress);
        display.push('\n');

        let mut sorted_tasks = tasks.clone();
        sorted_tasks.sort_by(|a, b| a.id.cmp(&b.id));

        for task in sorted_tasks {
            display.push_str(&format_task_display_with_options(&task, options));
        }

        display
    } else {
        String::new()
    }
}

fn format_tasks_complete_from_event(event: &TaskExecutionNotificationEvent) -> String {
    format_tasks_complete_from_event_with_options(event, TaskDisplayOptions::from_env())
}

fn format_tasks_complete_from_event_with_options(
    event: &TaskExecutionNotificationEvent,
    options: TaskDisplayOptions,
) -> String {
    if let TaskExecutionNotificationEvent::TasksComplete {
        stats,
        failed_tasks,
    } = event
    {
        let mut summary = String::new();
        push_display_line(&mut summary, options, "Execution Complete!");
        if options.plain {
            push_display_line(&mut summary, options, "-------------------");
        } else {
            push_display_line(&mut summary, options, "═══════════════════════");
        }

        push_display_line(
            &mut summary,
            options,
            format!("Total Tasks: {}", stats.total),
        );
        push_display_line(
            &mut summary,
            options,
            format!(
                "{}Completed: {}",
                glyph_prefix(options, "✅"),
                stats.completed
            ),
        );
        push_display_line(
            &mut summary,
            options,
            format!("{}Failed: {}", glyph_prefix(options, "❌"), stats.failed),
        );
        push_display_line(
            &mut summary,
            options,
            format!(
                "{}Success Rate: {:.1}%",
                glyph_prefix(options, "📈"),
                stats.success_rate
            ),
        );

        if !failed_tasks.is_empty() {
            summary.push('\n');
            push_display_line(
                &mut summary,
                options,
                format!("{}Failed Tasks:", glyph_prefix(options, "❌")),
            );
            for task in failed_tasks {
                let bullet = if options.plain { "-" } else { "•" };
                push_display_line(
                    &mut summary,
                    options,
                    format!("   {} {}", bullet, task.name),
                );
                if let Some(error) = &task.error {
                    push_display_line(
                        &mut summary,
                        options,
                        format!("     Error: {}", sanitize_inline(error)),
                    );
                }
            }
        }

        summary.push('\n');
        push_display_line(
            &mut summary,
            options,
            format!("{}Generating summary...", glyph_prefix(options, "📝")),
        );
        summary
    } else {
        String::new()
    }
}

fn format_task_display_with_options(task: &TaskInfo, options: TaskDisplayOptions) -> String {
    let mut task_display = String::new();

    push_display_line(
        &mut task_display,
        options,
        format!(
            "{} {} ({})",
            status_marker(&task.status, options),
            sanitize_inline(&task.task_name),
            sanitize_inline(&task.task_type)
        ),
    );

    if !task.task_metadata.is_empty() {
        push_display_line(
            &mut task_display,
            options,
            format!(
                "   {}Parameters: {}",
                glyph_prefix(options, "📋"),
                sanitize_inline(&task.task_metadata)
            ),
        );
    }

    if let Some(duration_secs) = task.duration_secs {
        push_display_line(
            &mut task_display,
            options,
            format!(
                "   {}{:.1}s",
                if options.plain { "time: " } else { "⏱️  " },
                duration_secs
            ),
        );
    }

    if matches!(task.status, TaskStatus::Running) && !task.current_output.trim().is_empty() {
        let processed_output = process_output_for_display(&task.current_output);
        if !processed_output.is_empty() {
            push_display_line(
                &mut task_display,
                options,
                format!(
                    "   {}{}",
                    if options.plain { "output: " } else { "💬 " },
                    sanitize_inline(&processed_output)
                ),
            );
        }
    }

    if matches!(task.status, TaskStatus::Completed) {
        if let Some(result_data) = &task.result_data {
            let result_preview = format_result_data_for_display(result_data);
            if !result_preview.is_empty() {
                push_display_line(
                    &mut task_display,
                    options,
                    format!(
                        "   {}{}",
                        if options.plain { "result: " } else { "📄 " },
                        sanitize_inline(&result_preview)
                    ),
                );
            }
        }
    }

    if matches!(task.status, TaskStatus::Failed) {
        if let Some(error) = &task.error {
            let error_preview = safe_truncate(error, 80);
            push_display_line(
                &mut task_display,
                options,
                format!(
                    "   {}{}",
                    if options.plain { "error: " } else { "⚠️  " },
                    sanitize_inline(&error_preview)
                ),
            );
        }
    }

    task_display.push('\n');
    task_display
}

fn status_marker(status: &TaskStatus, options: TaskDisplayOptions) -> &'static str {
    match (options.plain, status) {
        (true, TaskStatus::Pending) => "[pending]",
        (true, TaskStatus::Running) => "[running]",
        (true, TaskStatus::Completed) => "[done]",
        (true, TaskStatus::Failed) => "[failed]",
        (false, TaskStatus::Pending) => "⏳",
        (false, TaskStatus::Running) => "🏃",
        (false, TaskStatus::Completed) => "✅",
        (false, TaskStatus::Failed) => "❌",
    }
}

fn glyph_prefix(options: TaskDisplayOptions, glyph: &'static str) -> String {
    if options.plain {
        String::new()
    } else {
        format!("{glyph} ")
    }
}
