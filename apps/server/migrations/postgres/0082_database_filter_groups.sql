-- Docs databases: view filters become AND/OR groups. A filter's flat rule list
-- (`{"rules": [...]}`, all must match) becomes the root `and` group with the rules as its
-- children (`{"op": "and", "children": [...]}`).
UPDATE database_views
SET config_json = jsonb_set(
    config_json::jsonb,
    '{filter}',
    jsonb_build_object(
        'op', 'and',
        'children', coalesce(config_json::jsonb #> '{filter,rules}', '[]'::jsonb)
    )
)::text
WHERE jsonb_typeof(config_json::jsonb -> 'filter') = 'object'
  AND NOT (config_json::jsonb -> 'filter') ? 'op';
