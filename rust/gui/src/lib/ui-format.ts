export type ToolOutcome = "err" | "empty" | "ok";

export const baseName = (path: string): string => path.split(/[\\/]/).pop() || path;
export const formatTokens = (value: number): string => value >= 1000 ? `${(value / 1000).toFixed(1)}k` : `${value}`;
export const formatCost = (value: number): string => value >= 1 ? value.toFixed(2) : value.toFixed(4);
export const currencySymbol = (currency: "CNY" | "USD"): string => currency === "USD" ? "$" : "¥";
export const currencyName = (currency: "CNY" | "USD"): string => currency === "USD" ? "美元" : "人民币";
export const priceSourceName = (source: "official_direct" | "aggregator"): string => source === "official_direct" ? "厂商官方直连价" : "OpenRouter 聚合渠道价";

export function toolOutcome(result = ""): ToolOutcome {
  const exit = result.match(/Exit code: (-?\d+)/);
  const trimmed = result.trimStart();
  const body = result.replace(/\n?Exit code: -?\d+\s*$/, "").replace(/^STDERR:\s*/, "").trim();
  if (
    (exit && exit[1] !== "0") ||
    trimmed.startsWith("Error:") ||
    trimmed.startsWith("Sandbox denied:") ||
    trimmed.startsWith("[interrupted:")
  ) return "err";
  return body === "" ? "empty" : "ok";
}

export function toolStatusLabel(result = ""): string {
  const outcome = toolOutcome(result);
  if (outcome === "err") return "报错";
  if (outcome === "empty") return "无输出";
  return `${result.split("\n").length} 行`;
}

export function diffLineClass(line: string): string {
  if (line.startsWith("+++") || line.startsWith("---") || line.startsWith("diff ") || line.startsWith("index ")) return "dl-meta";
  if (line.startsWith("@@")) return "dl-hunk";
  if (line.startsWith("+")) return "dl-add";
  if (line.startsWith("-")) return "dl-del";
  return "";
}

const escapeHtml = (source: string) => source.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function inlineMarkdown(source: string): string {
  return source
    .replace(/`([^`]+)`/g, (_match, code) => `<code>${code}</code>`)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/__([^_]+)__/g, "<strong>$1</strong>")
    .replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>")
    .replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noreferrer">$1</a>');
}

export function renderMarkdown(source: string): string {
  const lines = (source || "").replace(/\r\n/g, "\n").split("\n");
  const output: string[] = [];
  let index = 0;
  let unordered = false;
  let ordered = false;
  const closeLists = () => {
    if (unordered) { output.push("</ul>"); unordered = false; }
    if (ordered) { output.push("</ol>"); ordered = false; }
  };
  const rowCells = (row: string) => row.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((cell) => cell.trim());
  while (index < lines.length) {
    const line = lines[index];
    if (/^```(\w*)\s*$/.test(line)) {
      closeLists();
      const buffer: string[] = [];
      index++;
      while (index < lines.length && !/^```\s*$/.test(lines[index])) buffer.push(lines[index++]);
      index++;
      output.push(`<pre class="md-code"><code>${escapeHtml(buffer.join("\n"))}</code></pre>`);
      continue;
    }
    if (/^\s*\|.*\|\s*$/.test(line) && index + 1 < lines.length && /^\s*\|?[\s:|-]+\|[\s:|-]*$/.test(lines[index + 1])) {
      closeLists();
      const headers = rowCells(line);
      index += 2;
      const rows: string[][] = [];
      while (index < lines.length && /^\s*\|.*\|\s*$/.test(lines[index])) rows.push(rowCells(lines[index++]));
      let table = '<table class="md-table"><thead><tr>' + headers.map((header) => `<th>${inlineMarkdown(escapeHtml(header))}</th>`).join("") + "</tr></thead><tbody>";
      for (const row of rows) table += "<tr>" + row.map((cell) => `<td>${inlineMarkdown(escapeHtml(cell))}</td>`).join("") + "</tr>";
      output.push(table + "</tbody></table>");
      continue;
    }
    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) { closeLists(); const level = heading[1].length; output.push(`<h${level} class="md-h">${inlineMarkdown(escapeHtml(heading[2]))}</h${level}>`); index++; continue; }
    if (/^\s*(---|\*\*\*|___)\s*$/.test(line)) { closeLists(); if (output.length && output.at(-1) !== "<hr/>") output.push("<hr/>"); index++; continue; }
    if (/^\s*>\s?/.test(line)) { closeLists(); output.push(`<blockquote>${inlineMarkdown(escapeHtml(line.replace(/^\s*>\s?/, "")))}</blockquote>`); index++; continue; }
    const unorderedItem = line.match(/^\s*[-*+]\s+(.*)$/);
    if (unorderedItem) { if (ordered) { output.push("</ol>"); ordered = false; } if (!unordered) { output.push("<ul>"); unordered = true; } output.push(`<li>${inlineMarkdown(escapeHtml(unorderedItem[1]))}</li>`); index++; continue; }
    const orderedItem = line.match(/^\s*\d+\.\s+(.*)$/);
    if (orderedItem) { if (unordered) { output.push("</ul>"); unordered = false; } if (!ordered) { output.push("<ol>"); ordered = true; } output.push(`<li>${inlineMarkdown(escapeHtml(orderedItem[1]))}</li>`); index++; continue; }
    if (line.trim() === "") { closeLists(); index++; continue; }
    closeLists();
    output.push(`<p>${inlineMarkdown(escapeHtml(line))}</p>`);
    index++;
  }
  closeLists();
  while (output.at(-1) === "<hr/>") output.pop();
  return output.join("\n");
}
