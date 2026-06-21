# Your first session

Welcome! This quick-start walks you through running your first interactive
session from the terminal.

## 1. Start a session

```bash
bharatcode
```

This drops you into an interactive chat. Type a request in plain language
(English or Hindi) and press Enter. For example:

```
summarize the files in this directory and suggest a README outline
```

## 2. Let it work

The assistant reads your project, proposes changes, and runs tools on your
behalf. You stay in control: review each step before approving anything that
edits files or runs commands.

## 3. Resume where you left off

Every session is saved automatically. To pick up your most recent one:

```bash
bharatcode --resume
```

## 4. Run a one-off task

Skip the interactive prompt and hand over a single instruction:

```bash
bharatcode run -t "add a unit test for the parser module"
```

## Next steps

- Configure a provider and model: `bharatcode configure`
- Browse the rest of these guides: `bharatcode tutorials`
- Read a specific guide: `bharatcode tutorials show <id>`

That's it — you've run your first session. Happy building!
