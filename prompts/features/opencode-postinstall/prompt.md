# OpenCode Postinstall

Keep npm and pnpm lifecycle scripts disabled by default in the sandbox. After
AGS installs the trusted `opencode-ai` package, explicitly run that package's
postinstall script and verify that the resulting `opencode` command executes.
