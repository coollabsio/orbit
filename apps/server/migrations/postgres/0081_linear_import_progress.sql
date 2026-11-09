-- What a running Linear import does, for its progress page. `stage`: reading (issue lists from
-- Linear), creating (tasks), details (files and comments) or linking (relations); NULL before the
-- start. `expected`: the issues the selected teams had at the scan. `reading`: issues read of the
-- team being listed (added to `total` once the team is stored). `waiting_until`: epoch ms when a
-- Linear rate limit lets the import go on, NULL when it is not waiting.
ALTER TABLE linear_imports ADD COLUMN stage TEXT CHECK (stage IN ('reading', 'creating', 'details', 'linking'));
ALTER TABLE linear_imports ADD COLUMN expected BIGINT NOT NULL DEFAULT 0 CHECK (expected >= 0);
ALTER TABLE linear_imports ADD COLUMN reading BIGINT NOT NULL DEFAULT 0 CHECK (reading >= 0);
ALTER TABLE linear_imports ADD COLUMN waiting_until BIGINT;
