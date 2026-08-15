import type { FieldKey } from './searchQuery';

export interface Tag {
  id: string;
  name: string;
}

export interface TagSummary extends Tag {
  paper_count: number;
  created_at: string;
}

export interface PaperSummary {
  id: string;
  /** Manual "known as" name (e.g. "RVSpec"); set only from the Details dock. */
  name: string | null;
  title: string | null;
  authors: string[];
  venue: string | null;
  year: number | null;
  doi: string | null;
  arxiv_id: string | null;
  dblp_key: string | null;
  cite_key: string | null;
  url: string | null;
  source: string | null;
  status: string;
  added_at: string;
  starred: boolean;
  tags: Tag[];
  projects: ProjectRef[];
}

export interface PaperDetail extends PaperSummary {
  abstract: string | null;
  summary: Summary | null;
}

export interface Summary {
  tldr: string;
  problem: string;
  approach: string;
  results: string;
  limitations: string;
}

export interface Project {
  id: string;
  name: string;
  paper_count: number;
}

/** A project as it rides on a paper row/detail — membership only, no count. */
export type ProjectRef = Pick<Project, 'id' | 'name'>;

export interface Stats {
  total: number;
  resolved: number;
  needs_review: number;
}

export type StatusFilter = 'all' | 'resolved' | 'needs_review';
export type Sort = 'year_desc' | 'year_asc' | 'added_desc' | 'title' | 'name';
export type BibFormat = 'bibtex' | 'biblatex';

export interface Filters {
  q: string;
  status: StatusFilter;
  sort: Sort;
  project: string;
  tag?: string;
  starred?: boolean;
}

export type ImportResult =
  | { outcome: 'ingested'; id: string; title: string | null; status: string }
  | { outcome: 'duplicate' }
  | { outcome: 'same_work'; id: string }
  | { outcome: 'in_trash'; id: string }
  | { outcome: 'unfetched'; title: string | null; doi: string | null };

export interface TranslateSettings {
  enabled: boolean;
  providers?: ('llm' | 'deepl')[];
  default_provider?: 'llm' | 'deepl';
  target_lang?: string;
  trigger?: 'auto' | 'manual';
}

export interface Settings {
  /** Host of the configured EZproxy; null when the deployment has no [proxy]
   *  (the import modal hides institutional access then). */
  proxy: { host: string } | null;
  proxy_cookie_set: boolean;
  proxy_cookie_updated_at: string | null;
  fold_abstract: boolean;
  translate?: TranslateSettings;
}

export interface Candidate {
  title: string | null;
  abstract: string | null;
  authors: string[];
  venue: string | null;
  year: number | null;
  doi: string | null;
  arxiv_id: string | null;
  dblp_key: string | null;
  url: string | null;
  source: string;
}

export type IdentifyBody =
  | { doi: string }
  | { arxiv_id: string }
  | { candidate: Candidate };

export interface SearchOpts {
  title: boolean;
  authors: boolean;
  abstract: boolean;
  body: boolean;
  /// The reader's own annotation notes.
  notes: boolean;
  keyword: boolean;
  semantic: boolean;
}

export interface SearchMatch {
  engine: 'keyword' | 'semantic' | 'both';
  /** The backend only ever emits FieldKey values; `string & {}` keeps an
   *  unknown future field assignable without collapsing the autocomplete. */
  field: FieldKey | (string & {});
  snippet: string;
  page: number | null;
}

export interface SearchResultItem {
  paper: PaperSummary;
  match: SearchMatch;
}

export interface SearchResponse {
  semantic: { available: boolean; reason: string | null };
  results: SearchResultItem[];
}

export interface TierCounts {
  indexed: number;
  pending: number;
  failed: number;
}

export interface SearchStatus {
  fts: TierCounts;
  vectors: TierCounts;
  semantic_available: boolean;
  reason: string | null;
}

/** The state of a paper's attached repository (wire format shared with
 *  the `PaperCode` struct in src/models.rs on the backend). */
export interface PaperCodeStatus {
  paper_id: string;
  repo_url: string;
  commit_sha: string | null;
  status: 'cloning' | 'ready' | 'error';
  error: string | null;
  cloned_at: string | null;
  size_bytes: number | null;
}

/** One reader annotation (wire format shared with the `Annotation` struct in
 *  src/annotations/mod.rs). `payload` is the plugin's own transfer item, stored
 *  verbatim so a field this app doesn't know about survives a round trip; it is
 *  `null` when the stored JSON failed to parse server-side. */
export interface Annotation {
  paper_id: string;
  id: string;
  page_index: number;
  kind: 'highlight' | 'underline' | 'strikeout' | 'squiggly' | 'text_comment';
  color: 'amber' | 'rose' | 'green' | 'blue' | 'violet';
  quoted_text: string | null;
  note: string | null;
  payload: unknown;
  created_at: string;
  updated_at: string;
}

/** The body of `PUT /api/papers/{id}/annotations/{annotation_id}`. The id is
 *  the path, not a field — that is what makes a retried save idempotent. */
export type NewAnnotation = Pick<
  Annotation,
  'page_index' | 'kind' | 'color' | 'quoted_text' | 'note' | 'payload'
>;

/** One selectable paper-chat model (wire format shared with the
 *  /api/chat/models response in src/web/chat.rs). */
export interface ChatModelInfo {
  id: string;
  label: string;
}

/** One stored chat turn as GET /api/papers/{id}/chat returns it (wire format
 *  shared with `ChatTurn` in src/web/dto.rs). `tools` arrives structured —
 *  the backend parses its stored tool log once; null when the turn used no
 *  tools (or the stored log was unparseable). */
export interface ChatTurnRow {
  id: number;
  role: 'user' | 'assistant';
  content: string;
  model: string | null;
  created_at: string;
  tools: { name: string; detail: string }[] | null;
}

/** One bibliography entry parsed to fields by [ai.citations] (wire format
 *  shared with src/citations/mod.rs on the backend). */
export interface StructuredReference {
  authors: string[];
  title: string | null;
  venue: string | null;
  year: number | null;
  doi: string | null;
  arxiv_id: string | null;
  url: string | null;
}
