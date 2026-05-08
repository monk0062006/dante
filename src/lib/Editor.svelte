<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState, type Extension } from "@codemirror/state";
  import {
    EditorView,
    Decoration,
    ViewPlugin,
    keymap,
    lineNumbers,
    highlightActiveLine,
    drawSelection,
    type DecorationSet,
    type ViewUpdate,
  } from "@codemirror/view";
  import {
    defaultKeymap,
    history,
    historyKeymap,
    indentWithTab,
  } from "@codemirror/commands";
  import { search, searchKeymap, highlightSelectionMatches } from "@codemirror/search";
  import {
    autocompletion,
    completionKeymap,
    type CompletionContext,
    type CompletionResult,
  } from "@codemirror/autocomplete";
  import {
    HighlightStyle,
    syntaxHighlighting,
    bracketMatching,
    indentOnInput,
  } from "@codemirror/language";
  import { json } from "@codemirror/lang-json";
  import { tags } from "@lezer/highlight";

  type Props = {
    value: string;
    onPaste?: (event: ClipboardEvent) => void;
    onRun?: () => void;
  };

  let { value = $bindable(""), onPaste, onRun }: Props = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let suppressNextSync = false;

  const danteTheme = EditorView.theme(
    {
      "&": {
        height: "100%",
        backgroundColor: "var(--bg)",
        color: "var(--text)",
        fontSize: "12px",
      },
      ".cm-content": {
        fontFamily: "var(--mono)",
        caretColor: "var(--accent)",
        padding: "12px 0",
      },
      ".cm-scroller": { fontFamily: "var(--mono)" },
      ".cm-gutters": {
        backgroundColor: "var(--bg-elev)",
        color: "var(--text-dim)",
        border: "none",
        borderRight: "1px solid var(--border)",
      },
      ".cm-activeLine": { backgroundColor: "rgba(139, 92, 246, 0.04)" },
      ".cm-activeLineGutter": {
        backgroundColor: "rgba(139, 92, 246, 0.06)",
        color: "var(--text)",
      },
      ".cm-selectionBackground, ::selection": {
        backgroundColor: "rgba(139, 92, 246, 0.35)",
      },
      "&.cm-focused .cm-selectionBackground": {
        backgroundColor: "rgba(139, 92, 246, 0.4)",
      },
      ".cm-cursor": { borderLeftColor: "var(--accent)" },
      ".cm-matchingBracket": {
        backgroundColor: "rgba(139, 92, 246, 0.2)",
        outline: "1px solid var(--accent-dim)",
      },
    },
    { dark: true },
  );

  const highlightStyle = HighlightStyle.define([
    { tag: tags.string, color: "#a3e635" },
    { tag: tags.number, color: "#fb923c" },
    { tag: tags.bool, color: "#f472b6" },
    { tag: tags.null, color: "#94a3b8" },
    { tag: tags.propertyName, color: "#7dd3fc" },
    { tag: tags.keyword, color: "#c4b5fd" },
    { tag: tags.comment, color: "#64748b", fontStyle: "italic" },
    { tag: tags.punctuation, color: "#94a3b8" },
  ]);

  const placeholderMark = Decoration.mark({ class: "cm-placeholder-var" });
  const requestLineMark = Decoration.mark({ class: "cm-request-line" });
  const headerKeyMark = Decoration.mark({ class: "cm-header-key" });
  const jsKeywordMark = Decoration.mark({ class: "cm-js-keyword" });
  const jsStringMark = Decoration.mark({ class: "cm-js-string" });
  const jsCommentMark = Decoration.mark({ class: "cm-js-comment" });
  const jsNumberMark = Decoration.mark({ class: "cm-js-number" });
  const sectionMark = Decoration.mark({ class: "cm-section" });

  const JS_KEYWORDS = new Set([
    "const", "let", "var", "if", "else", "for", "while", "do", "switch", "case",
    "break", "continue", "return", "function", "async", "await", "try", "catch",
    "finally", "throw", "new", "typeof", "instanceof", "in", "of", "true", "false",
    "null", "undefined", "this", "class", "extends", "super", "import", "export",
    "from", "default",
  ]);

  const dantePatterns = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = this.build(view);
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged) {
          this.decorations = this.build(update.view);
        }
      }
      build(view: EditorView): DecorationSet {
        return Decoration.set(
          this.decorationsList(view.state.doc.toString()).map((r) =>
            r.value.range(r.from, r.to),
          ),
          true,
        );
      }
      decorationsList(text: string): { from: number; to: number; value: Decoration }[] {
        const out: { from: number; to: number; value: Decoration }[] = [];

        const placeholder = /\{\{[\w.-]+\}\}/g;
        let m: RegExpExecArray | null;
        while ((m = placeholder.exec(text)) !== null) {
          out.push({ from: m.index, to: m.index + m[0].length, value: placeholderMark });
        }

        const lines = text.split("\n");
        let offset = 0;
        let inHeaders = false;
        let firstNonComment = true;
        let scriptMode: "pre" | "post" | null = null;
        for (const line of lines) {
          const trimmed = line.trimStart();
          const lead = line.length - trimmed.length;

          if (/^###/.test(trimmed)) {
            const lower = trimmed.toLowerCase();
            if (/^###\s*pre-?request\b/.test(lower)) scriptMode = "pre";
            else if (/^###\s*post-?request\b/.test(lower)) scriptMode = "post";
            else scriptMode = null;
            out.push({ from: offset, to: offset + line.length, value: sectionMark });
            inHeaders = false;
          } else if (scriptMode !== null) {
            // Highlight JS-like tokens in script lines
            // Comments
            const commentIdx = line.indexOf("//");
            const safeLine =
              commentIdx >= 0 ? line.slice(0, commentIdx) : line;
            if (commentIdx >= 0) {
              out.push({
                from: offset + commentIdx,
                to: offset + line.length,
                value: jsCommentMark,
              });
            }
            // Strings (quote-balanced; ignore complex escape edge cases)
            const stringRe = /"([^"\\]|\\.)*"|'([^'\\]|\\.)*'|`([^`\\]|\\.)*`/g;
            let sm: RegExpExecArray | null;
            while ((sm = stringRe.exec(safeLine)) !== null) {
              out.push({
                from: offset + sm.index,
                to: offset + sm.index + sm[0].length,
                value: jsStringMark,
              });
            }
            // Numbers
            const numRe = /\b\d+(\.\d+)?\b/g;
            while ((sm = numRe.exec(safeLine)) !== null) {
              out.push({
                from: offset + sm.index,
                to: offset + sm.index + sm[0].length,
                value: jsNumberMark,
              });
            }
            // Keywords
            const kwRe = /\b[a-z]+\b/g;
            while ((sm = kwRe.exec(safeLine)) !== null) {
              if (JS_KEYWORDS.has(sm[0])) {
                out.push({
                  from: offset + sm.index,
                  to: offset + sm.index + sm[0].length,
                  value: jsKeywordMark,
                });
              }
            }
          } else if (trimmed === "") {
            inHeaders = false;
          } else if (trimmed.startsWith("#")) {
            // skip
          } else if (firstNonComment) {
            const reqMatch = /^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+(\S+)/i.exec(trimmed);
            if (reqMatch) {
              out.push({
                from: offset + lead,
                to: offset + lead + reqMatch[0].length,
                value: requestLineMark,
              });
              inHeaders = true;
            }
            firstNonComment = false;
          } else if (inHeaders) {
            const colon = line.indexOf(":");
            if (colon > 0 && colon < line.length - 1) {
              out.push({ from: offset, to: offset + colon, value: headerKeyMark });
            }
          }
          offset += line.length + 1;
        }

        out.sort((a, b) => a.from - b.from || a.to - b.to);
        return out;
      }
    },
    { decorations: (v) => v.decorations },
  );

  const HEADER_NAMES = [
    "Accept", "Accept-Charset", "Accept-Encoding", "Accept-Language",
    "Authorization", "Cache-Control", "Connection", "Content-Encoding",
    "Content-Length", "Content-Type", "Cookie", "Date", "Host",
    "If-Match", "If-None-Match", "If-Modified-Since", "If-Unmodified-Since",
    "Origin", "Referer", "User-Agent", "X-API-Key", "X-Auth-Token",
    "X-Forwarded-For", "X-Request-Id", "X-Requested-With", "X-CSRF-Token",
    "Range", "Idempotency-Key", "Prefer", "Forwarded",
  ];

  const CONTENT_TYPE_VALUES = [
    "application/json",
    "application/x-www-form-urlencoded",
    "multipart/form-data",
    "application/xml",
    "text/plain",
    "text/html",
    "application/octet-stream",
    "application/graphql",
    "application/grpc-web+proto",
  ];

  function isHeaderPosition(state: EditorState, pos: number): boolean {
    const text = state.doc.toString();
    const before = text.slice(0, pos);
    const lines = before.split("\n");
    if (lines.length === 0) return false;
    let sawRequestLine = false;
    for (const line of lines.slice(0, -1)) {
      const trimmed = line.trim();
      if (!sawRequestLine) {
        if (trimmed === "" || trimmed.startsWith("#")) continue;
        if (/^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+\S+/i.test(trimmed)) {
          sawRequestLine = true;
          continue;
        }
        return false;
      } else {
        if (trimmed === "" || trimmed.startsWith("###")) return false;
      }
    }
    return sawRequestLine;
  }

  function danteCompletions(context: CompletionContext): CompletionResult | null {
    const line = context.state.doc.lineAt(context.pos);
    const lineStart = line.from;
    const beforeCursor = context.state.doc.sliceString(lineStart, context.pos);
    const colonIdx = beforeCursor.indexOf(":");

    if (colonIdx === -1) {
      // Header name completion
      const word = context.matchBefore(/[\w-]*/);
      if (!word) return null;
      if (!isHeaderPosition(context.state, context.pos)) return null;
      return {
        from: word.from,
        options: HEADER_NAMES.map((name) => ({
          label: name,
          type: "keyword",
          apply: `${name}: `,
        })),
        validFor: /^[\w-]*$/,
      };
    }

    // Value side
    const headerName = beforeCursor.slice(0, colonIdx).trim().toLowerCase();
    if (headerName === "content-type" || headerName === "accept") {
      const valueStart = lineStart + colonIdx + 1;
      const valueText = context.state.doc.sliceString(valueStart, context.pos).trim();
      const word = context.matchBefore(/[\w/+\-.*]*/);
      if (!word) return null;
      return {
        from: word.from,
        options: CONTENT_TYPE_VALUES.filter((v) =>
          v.toLowerCase().includes(valueText.toLowerCase()),
        ).map((v) => ({ label: v, type: "constant" })),
      };
    }

    return null;
  }

  function makeExtensions(): Extension[] {
    return [
      lineNumbers(),
      history(),
      drawSelection(),
      indentOnInput(),
      bracketMatching(),
      highlightActiveLine(),
      json(),
      syntaxHighlighting(highlightStyle),
      search({ top: true }),
      highlightSelectionMatches(),
      autocompletion({ override: [danteCompletions] }),
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        ...searchKeymap,
        ...completionKeymap,
        indentWithTab,
        {
          key: "Mod-Enter",
          run: () => {
            onRun?.();
            return true;
          },
        },
      ]),
      EditorView.lineWrapping,
      dantePatterns,
      danteTheme,
      EditorView.updateListener.of((vu) => {
        if (vu.docChanged) {
          const next = vu.state.doc.toString();
          if (next !== value) {
            suppressNextSync = true;
            value = next;
          }
        }
      }),
      EditorView.domEventHandlers({
        paste: (event) => {
          if (onPaste) onPaste(event);
          return false;
        },
      }),
    ];
  }

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({ doc: value, extensions: makeExtensions() }),
      parent: host,
    });
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  $effect(() => {
    if (!view) return;
    const current = view.state.doc.toString();
    if (suppressNextSync) {
      suppressNextSync = false;
      return;
    }
    if (current !== value) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
      });
    }
  });
</script>

<div class="editor-host" bind:this={host}></div>

<style>
  .editor-host {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  :global(.cm-placeholder-var) {
    background: rgba(139, 92, 246, 0.18);
    color: #c4b5fd;
    border-radius: 3px;
    padding: 0 2px;
    font-weight: 500;
  }

  :global(.cm-request-line) {
    color: #f9a8d4;
    font-weight: 600;
  }

  :global(.cm-header-key) {
    color: #7dd3fc;
  }

  :global(.cm-section) {
    color: var(--accent);
    font-weight: 600;
  }

  :global(.cm-js-keyword) {
    color: #c4b5fd;
    font-weight: 500;
  }

  :global(.cm-js-string) {
    color: #a3e635;
  }

  :global(.cm-js-comment) {
    color: #64748b;
    font-style: italic;
  }

  :global(.cm-js-number) {
    color: #fb923c;
  }
</style>
