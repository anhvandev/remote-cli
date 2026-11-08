# 01: Shared app shell and new sidebar

**What to build:** Prefactor the Dashboard so every signed-in page shares one app shell with the sidebar, instead of each page rendering its own navigation. The sidebar shows **Device** to every User and **Quản trị** to Admins only, and highlights the entry for the current page. For now Quản trị still opens the existing single Quản trị page. Nothing else changes for the user. This makes tickets 02, 03 and 06 easy: they only add routes inside the shell. See `.scratch/dashboard-redesign/spec.md` (Navigation, Implementation Decisions: shared app shell).

Assumes the shadcn restyle in progress on `feat/remote-cli` has been committed.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] One shared layout renders the sidebar for all signed-in pages; no page renders the sidebar itself
- [ ] The sign-in page has no sidebar
- [ ] Sidebar shows Device for everyone and Quản trị only for Admins; the current entry is a link with `aria-current="page"`
- [ ] Name, role and sign-out stay in the sidebar
- [ ] On narrow screens the sidebar still collapses to a top bar
- [ ] The whole existing Playwright suite passes unchanged (or with locator-only updates)
