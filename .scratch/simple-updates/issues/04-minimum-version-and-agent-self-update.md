# 04: Minimum Version and Agent self-update

**What to build:** A User sets an optional Minimum Version in Server settings, saved without checks. Devices whose Agent is below it show an "update needed · X < Y" badge on the Devices page and in `rcli devices`, and keep working normally. The Server sends an `AgentUpdate` for the Latest Build to such an Agent when all hold: a Minimum Version is set, the Agent is below it, and the Latest Build is above the Agent's version. It checks on Agent hello, when the Minimum Version is saved (to every connected Agent), and every hour per session while the condition holds. A failed self-update shows its reason under the badge. The reason clears when the Device reports a version at or above the Minimum Version, or when the Minimum Version is cleared or lowered to at or below the Device's version.

**Blocked by:** 01

**Status:** resolved

- [ ] The Minimum Version is editable on the Settings page and through the settings API; empty means none
- [ ] The Device JSON has `needs_update` and `update_error`; the Devices page badge and `rcli devices` render them
- [ ] An Agent below the Minimum Version receives the Latest Build and reconnects reporting the new version; an Agent at or above it receives nothing
- [ ] No `AgentUpdate` is sent when the Latest Build is not above the Agent's version
- [ ] Raising the Minimum Version reaches already connected Agents; a failure is retried after an hour
- [ ] A failed self-update shows `update_error`, which clears as described
- [ ] A Playwright e2e saves a Minimum Version and sees the badge on an outdated Device
