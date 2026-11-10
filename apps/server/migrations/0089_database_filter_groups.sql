-- Docs databases: view filters become AND/OR groups. A filter's flat rule list
-- (`{"rules": [...]}`, all must match) becomes the root `and` group with the rules as its
-- children (`{"op": "and", "children": [...]}`).
UPDATE database_views
SET config_json = json_set(
    config_json,
    '$.filter',
    json_object(
        'op', 'and',
        'children', json(coalesce(json_extract(config_json, '$.filter.rules'), '[]'))
    )
)
WHERE json_type(config_json, '$.filter') = 'object'
  AND json_type(config_json, '$.filter.op') IS NULL;
