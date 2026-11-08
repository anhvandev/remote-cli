# Spec: Dashboard redesign: one page per feature

Status: ready-for-agent

Vocabulary follows `GLOSSARY.md` (Device, Device Status, Agent, Controller, Control Lock, Live View, Enrollment Code, Operator, Admin). Builds on the shadcn restyle already in progress on `feat/remote-cli`; that restyle is committed first and is not part of this spec.

## Problem Statement

The Dashboard puts too much on too few pages. An Operator who opens a Device lands on one long page that mixes power buttons, Live View, the terminal, the Control Lock, current resources, hardware inventory and resource history, so finding the one thing they came for means scrolling past everything else. The fleet list has no way to find a Device by name or to narrow it to the Offline ones. An Admin gets a single Quản trị page with five unrelated blocks (Users & API tokens, Enrollment Codes, unlock passwords, Agent updates, Device management) and no quick answer to "is anything wrong right now?".

## Solution

Split the Dashboard into one page per feature, with navigation that tells the user where they are:

- A left sidebar with **Device** for everyone and **Quản trị** (Admin only) with four sub-entries.
- The fleet list keeps its table and gains a name search and a Device Status filter, both kept in the URL.
- Each Device gets four tabs under a shared header: **Tổng quan**, **Màn hình**, **Terminal**, **Lịch sử**. The header (name, Device Status, power buttons, Control Lock reminder, tabs) stays the same on every tab.
- Quản trị becomes an overview page of four cards (Users, Devices, Agent updates, Enrollment Codes), each leading to its own sub-page.

The Operator is the primary user. Admin pages only need to be tidy and clear.

## User Stories

### Navigation

1. As an Operator, I want a sidebar entry for Device, so that I can always get back to the fleet list.
2. As an Operator, I want not to see any Quản trị entry, so that the navigation only shows what I can use.
3. As an Admin, I want a Quản trị entry in the sidebar that opens the admin overview, so that I can see the state of the system at a glance.
4. As an Admin, I want four sub-entries under Quản trị (User, Device, Enrollment Code, Cập nhật Agent), so that I can jump straight to a task.
5. As any User, I want the current sidebar entry highlighted, so that I know where I am.
6. As any User, I want my name, role and a sign-out button to stay in the sidebar, so that I can sign out from any page.
7. As any User on a phone, I want the layout not to break, so that I can still check the fleet list and a Device's overview.

### Fleet list

8. As an Operator, I want to search Devices by name, so that I can find one Device in a large fleet quickly.
9. As an Operator, I want to filter the list by Device Status (Online, Offline, Waking), so that I can see which Devices need attention.
10. As an Operator, I want the counts of Online, Offline and Waking Devices to stay visible, so that I know the fleet's health without filtering.
11. As an Operator, I want the search text and status filter kept in the URL, so that pressing Back from a Device returns me to the same filtered list.
12. As an Operator, I want to share a link to a filtered list with a colleague, so that we look at the same Devices.
13. As an Operator, I want a clear empty message when no Device matches my search or filter, so that I don't mistake it for an empty fleet.
14. As an Operator, I want clicking a Device row to open its Tổng quan tab, so that I land on the summary first.
15. As an Operator, I want the list to keep showing Agent version, pending or failed Agent updates, revoked Devices and last-seen time, so that I lose nothing from today's table.

### Device: shared header and tabs

16. As an Operator, I want the Device's name and Device Status at the top of every tab, so that I always know which Device I am working on and whether it is reachable.
17. As an Operator, I want the power buttons (Wake, restart, …) in the header on every tab, so that I can wake an Offline Device without hunting for the button.
18. As an Operator, I want four tabs (Tổng quan, Màn hình, Terminal, Lịch sử) each with its own URL, so that I can bookmark or share a specific view of a Device.
19. As an Operator, I want the active tab marked, so that I know which view I'm on.
20. As an Operator holding the Control Lock, I want a "Bạn đang giữ Control Lock · Nhả" reminder in the header on every tab, so that I don't forget I'm blocking other Controllers.
21. As an Operator holding the Control Lock, I want to release it from that reminder on any tab, so that I don't have to go back to Màn hình to let others in.
22. As an Operator, I want switching tabs not to release my Control Lock, so that I can glance at Tổng quan and come back to keep controlling.
23. As an Operator, I want switching tabs not to drop my watch on the Control Lock, so that the reminder and the Màn hình tab stay accurate.

### Device: Tổng quan tab

24. As an Operator, I want current resources (CPU, GPU, RAM, disks, network, temperature, battery, uptime) on Tổng quan, so that I can see how the Device is doing.
25. As an Operator, I want the screen state (logged in, lock screen, login screen) and logged-in user on Tổng quan, so that I know what I'll find on Màn hình.
26. As an Operator, I want the hardware inventory (OS, hostname, CPU, RAM, GPUs, disks, network interfaces, displays, serial) on Tổng quan, so that I can identify the machine.
27. As an Operator, I want Tổng quan to still show inventory when the Device is Offline, so that I can identify a Device I can't reach.
28. As an Operator, I want a clear note that current resources are only available while Online, so that empty values don't look like a fault.

### Device: Màn hình tab

29. As an Operator, I want the Live View and the Control Lock controls on Màn hình, so that watching and controlling the screen is one focused page.
30. As an Operator, I want to take, wait for and release the Control Lock on Màn hình exactly as today, so that the Control Lock rules don't change.
31. As an Operator, I want the Live View to stop when I leave Màn hình, so that it doesn't use bandwidth on a tab I'm not looking at.
32. As an Operator viewing an Offline Device, I want Màn hình to say the Device is Offline and offer Wake, so that I know what to do instead of seeing a broken player.
33. As an Operator viewing a Waking Device, I want Màn hình to say it is waiting for the Device to come Online, so that I know the Wake is in progress.

### Device: Terminal tab

34. As an Operator, I want the terminal on its own tab, so that it has the full width of the page.
35. As an Operator who needs the screen and the terminal at once, I want to open the two tabs in two browser tabs, so that I can still work on both.
36. As an Operator viewing an Offline Device, I want Terminal to say the Device is Offline and offer Wake.
37. As an Operator viewing a Waking Device, I want Terminal to say it is waiting for the Device to come Online.

### Device: Lịch sử tab

38. As an Operator, I want the resource history charts on Lịch sử with the existing range choices, so that I can investigate past behaviour.
39. As an Operator, I want the selected range kept in the URL, so that Back and shared links keep it.
40. As an Operator, I want Lịch sử to work for Offline Devices, so that I can look at history after a Device goes down.

### Quản trị: overview

41. As an Admin, I want /admin to show four cards (User, Device, Cập nhật Agent, Enrollment Code), so that I see the whole system on one screen.
42. As an Admin, I want the User card to show how many Admins and Operators there are, how many Users are disabled and how many API tokens exist.
43. As an Admin, I want the Device card to show Online, Offline and Waking counts, the number of revoked Devices and the number of Devices without an unlock password, so that I spot gaps in setup.
44. As an Admin, I want the Cập nhật Agent card to show, for each version being rolled out, how many Devices are updated out of the total and how many failed.
45. As an Admin, I want failed Agent updates listed with the Device name and error, linking to that Device's Tổng quan, so that I can investigate straight away.
46. As an Admin, I want the Enrollment Code card to offer "Tạo Enrollment Code", so that adding a new Device starts from the overview.
47. As an Admin, I want each card to lead to its sub-page, so that I can act on what I see.

### Quản trị: sub-pages

48. As an Admin, I want a User page to create Users, set Dashboard passwords, issue and revoke API tokens, and disable Users, exactly as today.
49. As an Admin, I want a Device page to rename, merge history, and revoke Devices, and to set or change each Device's unlock password, so that all per-Device settings are in one place.
50. As an Admin, I want an Enrollment Code page to create Enrollment Codes, exactly as today.
51. As an Admin, I want a Cập nhật Agent page to upload a build and roll it out to chosen Devices, and see rollout progress, exactly as today.
52. As an Operator, I want any /admin page I open by URL to tell me it is Admin-only and show no admin data.

## Implementation Decisions

- **Frontend only.** Everything is built in the Dashboard (Next.js App Router) against the Server's existing Controller API. No Server, Agent or API change. Anything that turns out to need a new API is written up as a separate ticket instead of being built here.
- **Shared app shell.** The sidebar moves into a layout shared by all signed-in pages instead of being rendered by each page. It renders Device for everyone and Quản trị plus four sub-entries for Admins only, and marks the current entry.
- **Routes.**
  - Fleet list stays at the root; it reads search text and status from query parameters (`q`, `status`) and filters the already-fetched Device list. The filter controls update the URL rather than holding local state.
  - Device tabs are nested routes under the Device: Tổng quan at the Device's root, then `screen`, `terminal`, `history`. The Lịch sử range keeps its existing query parameter.
  - Quản trị: overview at `/admin`, sub-pages `users`, `devices`, `enrollment`, `updates`.
- **Device layout.** A layout shared by the four Device tabs fetches the Device, its Control Lock and the current User once and renders the header: name, Device Status, power buttons, Control Lock reminder, tab bar. The Control Lock provider (the component that watches the lock and exposes acquire/release) lives in this layout so it survives tab switches; tab pages read lock state from it. A 404 from the Server still shows the not-found page.
- **Control Lock behaviour is unchanged.** Leaving Màn hình does not release the lock; the lock still ends only per the glossary rules (holder releases, 10 minutes without an Action, 60 seconds disconnected, Admin force-release). The header reminder shows only while the current User is the holder and offers release.
- **Session lifetime.** Live View and terminal sessions keep their current behaviour of closing when their component unmounts, so leaving Màn hình stops the Live View and leaving Terminal closes the terminal session. This is accepted, not worked around.
- **Offline/Waking placeholder.** Màn hình and Terminal render, in place of their content, a message for Offline (with a Wake button using the existing power action) or Waking (waiting for Online). Tabs are never hidden or disabled.
- **Admin overview data** comes from data the old Quản trị page already fetched: Users, API tokens, Devices (status, revoked, per-Device Agent update status and error) and per-Device unlock password status. Rollout progress is grouped by version the same way the old page did. There is no API to list Enrollment Codes, so that card shows no count.
- **Admin sub-pages** move the existing forms and server actions unchanged; only their placement changes. The unlock password block joins the Device sub-page.
- **Admin-only guard.** Every page under Quản trị keeps the existing behaviour for Operators: a "Chỉ admin" message and no admin data. The guard lives in the Quản trị layout so it covers every sub-page.
- **Accessible names are part of the contract.** Sections keep their region labels, tabs and sidebar entries are links with their visible names, and the active one carries `aria-current="page"`, because the end-to-end tests locate elements by role and name.
- **Language.** Glossary terms stay in English (Device, Agent, Control Lock, Live View, Enrollment Code, Operator, Admin); all other copy is Vietnamese.
- **Responsive.** Desktop first; on narrow screens the sidebar collapses to a top bar (as today) and the fleet list and Tổng quan must not break.

## Testing Decisions

- **One seam: the Playwright end-to-end suite** running against `rcli-devstack` (real Server on Postgres in a test container, FakePlatform Agents, manual clock). Tests drive the Dashboard as a user would and assert on what is visible, located by role and accessible name, never on component internals or CSS.
- **Update the existing specs** (sign-in and fleet, live view, direct control, terminal, history, power and wake, admin) for the new structure: after opening a Device, click the relevant tab; in admin tests, go through the overview or sidebar sub-entry before using a section.
- **New tests:**
  - Fleet search and status filter narrow the list, survive Back from a Device, and come from the URL when opened directly; no-match shows the empty message.
  - Device tabs: clicking a Device lands on Tổng quan; each tab has its own URL and is marked active.
  - Taking the Control Lock on Màn hình, then switching to Tổng quan, shows the header reminder and the lock is still held; releasing from the reminder frees it.
  - An Offline Device's Màn hình and Terminal show the Offline message with Wake, and Wake moves it to Waking.
  - The admin overview shows the four cards with counts matching the devstack fixtures, and a failed Agent update is listed with a link to that Device.
  - An Operator opening each Quản trị URL sees only the Admin-only message.
- **Prior art:** existing specs share sign-in, user creation and admin API helpers through the devstack helper module. Reuse them. Devices reserved for specific tests (as the admin spec reserves its own) stay reserved.
- No unit or component tests: there is no prior art for them in the Dashboard and no pure logic worth isolating.

## Out of Scope

- Any change to the Server, Agent, CLI or Controller API, including an API to list Enrollment Codes.
- Keeping a terminal session or Live View alive across tab switches, or showing both on one page.
- A home page for all Users other than the fleet list.
- Card or thumbnail views of the fleet, bulk actions on Devices, saved filters.
- Charts on the admin overview.
- New visual design work beyond the shadcn restyle already in progress.
- Mobile-specific layouts for Màn hình and Terminal.

## Further Notes

- The work is naturally three slices that can land in order: (1) app shell, sidebar and fleet search/filter; (2) Device layout and tabs; (3) admin overview and sub-pages.
- Glossary terms Live View, Operator and Admin were added during the design session.
