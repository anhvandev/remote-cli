-- Install packages (.dmg, .deb, .rpm) an Admin uploads for the Connect guide's downloads: one per kind and arch,
-- replaced by the next upload. Unlike Agent builds they are not signed: a person installs them, the OS checks them.
CREATE TABLE install_packages (
    kind TEXT NOT NULL,
    arch TEXT NOT NULL,
    package BYTEA NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL,
    uploaded_by TEXT NOT NULL,
    PRIMARY KEY (kind, arch)
);
