# Security

PlumCraft is pre-release software with no supported versions yet. Fixes go to the main branch.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting (the Security tab, "Report a vulnerability"), not a public issue. Include the steps to reproduce it and what an attacker gains.

## What PlumCraft assumes

- **Engines are hostile to parsers, not to your machine.** Every message from an engine is parsed as hostile input, and parsing never panics. An engine is still a program you chose to run, with your permissions, so only run engines you trust.
- **Workflows are programs.** A workflow can read and write files through nodes such as Load Files and Write Files, with your permissions. Opening one runs nothing. The first run of a workflow from elsewhere stops and shows what it reads, writes and does, and it goes ahead only if you approve it. The review can't show what a craft app does inside its own nodes, or a path that is computed during the run; it says when another node sets a path. There is no sandbox, so approving a workflow gives it your permissions. Pasting nodes from the clipboard isn't reviewed.
- **The command line counts as approval.** `plumcraft-editor run FILE` and `--screenshot` run the file you named without asking.
- **Remote engines see what you send them.** Files that a graph uses are uploaded to an HTTP engine. The editor asks before it sends files to an engine you attached to, and an engine can't ask for a file through a default value. After you allow a file, later runs send it without asking until you disconnect.

## Network

- **The control channel** is off unless you pass `--control`. It listens on loopback and needs a token. A token file must be a plain file of yours that nobody else can reach.
- **HTTP engines** always need a bearer token and won't start without one, on loopback too. A web page you have open can reach loopback, so a local address protects nothing by itself.
- **There is no built-in TLS.** To reach a remote engine, use an SSH tunnel or a TLS proxy.
