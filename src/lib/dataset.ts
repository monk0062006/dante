export interface Dataset {
  columns: string[];
  rows: string[][];
}

export function parseDataset(text: string): Dataset | null {
  const lines = text.split(/\r?\n/);
  let i = 0;
  let inData = false;
  const datasetLines: string[] = [];
  for (const rawLine of lines) {
    const line = rawLine.trimEnd();
    const trimmed = line.trim();
    if (/^###\s*data\b/i.test(trimmed)) {
      inData = true;
      continue;
    }
    if (trimmed.startsWith("###")) {
      inData = false;
      continue;
    }
    if (!inData) continue;
    if (trimmed.startsWith("#")) continue;
    if (!trimmed) {
      // allow blank lines inside the block? skip
      continue;
    }
    datasetLines.push(line);
  }
  if (datasetLines.length === 0) return null;

  // First line is the header.
  const header = parseCsvLine(datasetLines[0]);
  if (header.length === 0) return null;
  const rows: string[][] = [];
  for (let r = 1; r < datasetLines.length; r++) {
    const cells = parseCsvLine(datasetLines[r]);
    if (cells.length === 0) continue;
    // Pad or truncate to header length
    while (cells.length < header.length) cells.push("");
    if (cells.length > header.length) cells.length = header.length;
    rows.push(cells);
  }
  if (rows.length === 0) return null;
  return { columns: header, rows };
}

function parseCsvLine(line: string): string[] {
  const out: string[] = [];
  let buf = "";
  let inQuotes = false;
  let i = 0;
  while (i < line.length) {
    const ch = line[i];
    if (inQuotes) {
      if (ch === '"') {
        if (line[i + 1] === '"') {
          buf += '"';
          i += 2;
          continue;
        }
        inQuotes = false;
        i++;
        continue;
      }
      buf += ch;
      i++;
    } else {
      if (ch === '"' && buf === "") {
        inQuotes = true;
        i++;
      } else if (ch === ",") {
        out.push(buf);
        buf = "";
        i++;
      } else {
        buf += ch;
        i++;
      }
    }
  }
  out.push(buf);
  // Trim each cell
  return out.map((c) => c.trim());
}

export function rowToVars(dataset: Dataset, rowIdx: number): Map<string, string> {
  const m = new Map<string, string>();
  if (rowIdx < 0 || rowIdx >= dataset.rows.length) return m;
  const row = dataset.rows[rowIdx];
  for (let i = 0; i < dataset.columns.length; i++) {
    m.set(dataset.columns[i], row[i] ?? "");
  }
  return m;
}
