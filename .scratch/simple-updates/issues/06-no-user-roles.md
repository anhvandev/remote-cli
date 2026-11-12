# 06: No User roles

**What to build:** Every User may do everything (ADR 0009): manage Users, the Server Key, Server settings, Agent Builds and Devices, and force-release a Control Lock. The role column and the Admin check go away. A Controller connected by Device Password stays restricted to its one Device on every route that used to be admin-only. Any User's `https://` Dashboard visit can record the API Address. A User cannot delete or disable themselves or the last enabled User. The Users form loses the role field, and the "Admin" nav becomes "Quản lý" (Users, Updates, Settings), visible to every User.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [ ] A former Operator can perform every former admin-only action; existing Operators need no migration step
- [ ] A Device-Password caller is refused on every former admin-only route
- [ ] Deleting or disabling oneself, or the last enabled User, is refused with a clear message
- [ ] The API Address is learned from any User's Dashboard request
- [ ] The Users form has no role; the nav shows "Quản lý" to every User
- [ ] The existing auth and admin-device tests are extended for the above
