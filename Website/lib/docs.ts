import "server-only";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { allDocs } from "./docs-catalog";
import { buildApiMarkdown } from "./api-docs";

export function findDoc(slugParts?: string[]) {
  const slug = slugParts?.join("/") ?? "";
  return allDocs.find((doc) => doc.slug === slug);
}

export async function readDoc(source: string) {
  if (source.startsWith("api:")) return buildApiMarkdown(source.slice(4));
  const engineRoot = path.resolve(process.cwd(), "..", "Engine");
  return readFile(path.join(engineRoot, source), "utf8");
}
