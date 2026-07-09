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
}

export interface PaperDetail extends PaperSummary {
  abstract: string | null;
  project_ids: string[];
}

export interface Project {
  id: string;
  name: string;
  note: string | null;
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
}

export type ImportResult =
  | { outcome: 'ingested'; id: string; title: string | null; status: string }
  | { outcome: 'duplicate' }
  | { outcome: 'same_work'; id: string }
  | { outcome: 'in_trash'; id: string }
  | { outcome: 'unfetched'; title: string | null; doi: string | null };

export interface Settings {
  proxy_cookie_set: boolean;
  proxy_cookie_updated_at: string | null;
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
