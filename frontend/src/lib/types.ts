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
  tags: { id: string; name: string }[];
  projects: { id: string; name: string }[];
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

export interface Stats {
  total: number;
  resolved: number;
  needs_review: number;
}

export type StatusFilter = 'all' | 'resolved' | 'needs_review';
export type Sort = 'year_desc' | 'year_asc' | 'added_desc' | 'title';
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
  keyword: boolean;
  semantic: boolean;
}

export interface SearchMatch {
  engine: 'keyword' | 'semantic' | 'both';
  field: string;
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
