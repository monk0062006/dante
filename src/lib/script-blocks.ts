export interface ScriptBlocks {
  preScript: string | null;
  postScript: string | null;
}

export function parseScriptBlocks(text: string): ScriptBlocks {
  const lines = text.split(/\r?\n/);
  let pre: string[] | null = null;
  let post: string[] | null = null;
  let current: "pre" | "post" | null = null;

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();
    if (/^###\s*pre-?request\b/i.test(trimmed)) {
      current = "pre";
      pre = [];
      continue;
    }
    if (/^###\s*post-?request\b/i.test(trimmed)) {
      current = "post";
      post = [];
      continue;
    }
    if (trimmed.startsWith("###")) {
      current = null;
      continue;
    }
    if (current === "pre" && pre) pre.push(rawLine);
    if (current === "post" && post) post.push(rawLine);
  }

  return {
    preScript: pre ? pre.join("\n").trim() : null,
    postScript: post ? post.join("\n").trim() : null,
  };
}
