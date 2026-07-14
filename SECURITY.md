# Security policy

BharatCode is a developer agent that can read files, call network services, and execute tools with
the user's privileges. Treat model output and content retrieved from repositories, websites, and
extensions as untrusted.

## Safe operation

- Keep the default SmartApprove mode and review sensitive tool arguments before approving them.
- Use the sandbox, offline, residency, and egress controls appropriate to the task.
- Run against a least-privileged account and avoid placing unrelated secrets in the working tree
  or process environment.
- Review extension commands and packages before allowing them; an `stdio` extension is executable
  code.
- Require a nonempty server secret for non-loopback binds and protect that secret like a password.
- Review generated code and tests before committing or deploying them.

Prompt injection cannot be eliminated solely by an LLM's instructions. Isolate high-risk work,
limit credentials and network reachability, and verify consequential actions independently.

## Reporting a vulnerability

Report vulnerabilities privately through this repository's GitHub Security tab using “Report a
vulnerability.” Include affected versions, impact, reproduction steps, and any proposed mitigation.
Do not publish exploit details before maintainers have had a reasonable opportunity to validate and
coordinate a fix.

Maintainers will acknowledge the report, investigate it, coordinate remediation and disclosure,
and credit the reporter when requested and appropriate.
