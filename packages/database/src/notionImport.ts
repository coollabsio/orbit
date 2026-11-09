import type { NotionImport } from "@orbit/api/generated/types.gen";

type Report = Pick<
  NonNullable<NotionImport["report"]>,
  "databases_imported" | "rows_truncated" | "skipped" | "lossy"
>;

const count = (n: number, one: string, many: string) =>
  `${n} ${n === 1 ? one : many}`;

/** "calendar (2), board (1)": most first, then by name. */
const listed = (kinds: [string, number][]) =>
  kinds
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([kind, n]) => `${kind.replaceAll("_", " ")} (${n})`)
    .join(", ");

/** Sentences for the database reasons of `report.lossy`, by key. */
const LOSSY: Record<string, (n: number) => string> = {
  formula_values: (n) =>
    `${count(n, "formula property was", "formula properties were")} imported as fixed values (Orbit does not recompute them).`,
  rollup_values: (n) =>
    `${count(n, "rollup property was", "rollup properties were")} imported as fixed values (Orbit does not recompute them).`,
  relation_titles: (n) =>
    `${count(n, "relation property was", "relation properties were")} imported as text with the titles of the related pages.`,
  files_as_names: (n) =>
    `${count(n, "files property was", "files properties were")} imported as text with the file names.`,
  number_format: (n) =>
    `${count(n, "number property uses", "number properties use")} a format Orbit does not have (such as Bitcoin) and shows plain numbers.`,
  people_as_text: (n) =>
    n === 1
      ? "1 created-by or edited-by property names people who are not Orbit members, so it was imported as text."
      : `${n} created-by or edited-by properties name people who are not Orbit members, so they were imported as text.`,
  unmatched_people: (n) =>
    n === 1
      ? "1 person in a person property has no Orbit member with the same email and was left out."
      : `${n} people in person properties have no Orbit member with the same email and were left out.`,
  invalid_values: (n) =>
    n === 1
      ? "1 value does not fit its Orbit property (for example a link that is not a web address) and was left out."
      : `${n} values do not fit their Orbit property (for example a link that is not a web address) and were left out.`,
  values_truncated: (n) =>
    `${count(n, "value was", "values were")} cut to Orbit's limits (10,000 characters, 100 options or people).`,
  options_truncated: (n) =>
    `${count(n, "property had", "properties had")} more than 200 options; the others were left out.`,
  properties_truncated: (n) =>
    `${count(n, "property was", "properties were")} left out: an Orbit database holds at most 100.`,
  view_filter: (n) =>
    `${count(n, "view lost", "views lost")} part of the filter: Orbit keeps conditions that all must match, without OR groups or conditions it does not have.`,
  view_sort: (n) =>
    `${count(n, "view lost", "views lost")} a sort by something Orbit does not sort by (such as the creation time of a row).`,
  views_unavailable: (n) =>
    `${count(n, "database got", "databases got")} one table view because Notion did not return the views.`,
};

/**
 * The conversion notes of the database part of a Notion import: sentences for the known
 * database reasons, and the other skipped and lossy kinds for the generic lists.
 */
export function notionDatabaseNotes(report: Report): {
  notes: string[];
  skipped: Record<string, number>;
  lossy: Record<string, number>;
} {
  const notes: string[] = [];
  if (report.databases_imported > 0)
    notes.push(
      `${count(report.databases_imported, "Notion database became an Orbit database", "Notion databases became Orbit databases")} with properties, views and rows.`,
    );
  if (report.rows_truncated > 0)
    notes.push(
      `${count(report.rows_truncated, "row was", "rows were")} not imported: an Orbit database holds at most 5,000 rows.`,
    );
  const lossy: Record<string, number> = {};
  const viewTypes: [string, number][] = [];
  for (const [kind, n] of Object.entries(report.lossy)) {
    if (n <= 0) continue;
    const sentence = LOSSY[kind];
    if (sentence) notes.push(sentence(n));
    else if (kind.startsWith("view_type:"))
      viewTypes.push([kind.slice("view_type:".length), n]);
    else lossy[kind] = n;
  }
  if (viewTypes.length > 0)
    notes.push(
      `Views imported as tables (Orbit has table and board views only): ${listed(viewTypes)}.`,
    );
  const skipped: Record<string, number> = {};
  const properties: [string, number][] = [];
  for (const [kind, n] of Object.entries(report.skipped)) {
    if (n <= 0) continue;
    if (kind.startsWith("property:"))
      properties.push([kind.slice("property:".length), n]);
    else skipped[kind] = n;
  }
  if (properties.length > 0)
    notes.push(
      `Skipped database properties (not supported in Orbit): ${listed(properties)}.`,
    );
  return { notes, skipped, lossy };
}
