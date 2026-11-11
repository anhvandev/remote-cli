# Access by Server Key and Device ID + Device Password instead of Enrollment Codes and API tokens

Setup took too many steps: an admin had to create an Enrollment Code for every Device, Agents needed a CA file, a fingerprint and an update key, and every Controller needed a hand-made API token in env vars. We switch to the TeamViewer/RustDesk model. A new install needs only the Server's address and one Server Key. Any machine holding the key registers itself and gets a 9-digit Device ID plus a static Device Password. A Controller reaches a Device with its Device ID and Device Password, and needs no User account. The Server Key bundles the Server CA fingerprint, which pins the Server's identity on first contact, with a shared secret that grants permission to connect. Replacing the key changes only the secret, so registered Devices keep working with their per-Device mTLS certificates. Dashboard Users still sign in and reach every Device without its password.

## Considered Options

- Keep Users for the CLI and use the Device Password only to add a Device to a User's list: audit stays tied to a person, but the CLI still needs a login and the "just an ID and password" flow is lost. Rejected.
- Require both a User login and the Device Password: the most secure, but the most steps. Rejected.

## Consequences

- Audit and the Control Lock holder name a password connection by the Controller's machine name and IP, not by a person.
- The Server Key's secret is copied to every install and ends up in shell history, so it is a weak gate. A leaked key lets anyone register fake Devices and try passwords, but not control a Device. The real protection is the Device Password, which the Server rate-limits per Device ID and IP, plus the per-Device mTLS certificate.
- The update-signing key is pinned when an Agent registers, or when `rcli setup` runs on a Controller, over the channel verified by the fingerprint. A later Server compromise therefore cannot push updates signed with a different key.
- The fingerprint pins the CA only where the Server presents it: the Agent port, at registration and on every connection. The Controller API is served behind a TLS reverse proxy with its own certificate, so `rcli setup` cannot pin the CA there (the CA certificate is public, so fetching it proves nothing). It requires an `https://` Server Address instead (plain `http://` is accepted only for a Server on the same machine, which also keeps the tests hermetic), and trusts the update key only through its HMAC under the secret.
- The secret gates registration and password connections only. A User session does not present it: any signed-in User can read the Server Key on the Connect guide, so requiring it again would add a step and no protection.
