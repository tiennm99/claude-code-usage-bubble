# TODO

Steps left before the Tauri v2 port on `feat/tauri-port` can ship. Do them in
order.

1. **Finish the updater signing key.** The key pair exists, its public key is
   in `tauri.conf.json`, and the `TAURI_SIGNING_PRIVATE_KEY` secret is set.
   Still to do: add the `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secret, and keep
   an offline backup of `usage-monitor.key` and its password. Losing either
   leaves installed copies unable to update.
2. **Test on Windows 10 and 11.** Download the exe artifact from the latest CI
   run on `feat/tauri-port` and work through the checklist in
   [the final port review](plans/reports/code-reviewer-261021-1028-tauri-port-final.md).
   Check toast notifications first: a freshly downloaded exe may not show them.
3. **Run the release dry run.** Follow the dry-run steps in
   [docs/release-process.md](docs/release-process.md). It creates a draft
   release and deletes it afterwards.
4. **Merge into `main`.** This reaches production: the next tag publishes the
   Tauri build to existing users through self-update.
