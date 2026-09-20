"use client";

import { ArrowLeft, ArrowRight, Check, ChevronDown, Command, Copy, Github, Menu, Moon, Search, Sun, X } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { allDocs, docGroups, routeFor } from "@/lib/docs-catalog";

function textSlug(text: string) {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

function resolveDocHref(href: string) {
  if (/^(https?:|mailto:|#)/.test(href)) return href;
  const clean = href.split("#")[0].replaceAll("\\", "/");
  const filename = clean.split("/").pop()?.toLowerCase();
  const match = allDocs.find((doc) => doc.source.toLowerCase().endsWith(clean.toLowerCase()) || doc.source.split("/").pop()?.toLowerCase() === filename);
  return match ? routeFor(match.slug) : href;
}

function inline(text: string): ReactNode[] {
  const pattern = /(\[([^\]]+)\]\(([^)]+)\)|`([^`]+)`|\*\*([^*]+)\*)/g;
  const nodes: ReactNode[] = [];
  let cursor = 0;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(text))) {
    if (match.index > cursor) nodes.push(text.slice(cursor, match.index));
    if (match[2] && match[3]) {
      const href = resolveDocHref(match[3]);
      nodes.push(/^https?:/.test(href) ? <a href={href} target="_blank" rel="noreferrer" key={`${match.index}-a`}>{match[2]}</a> : <Link href={href} key={`${match.index}-l`}>{match[2]}</Link>);
    } else if (match[4]) nodes.push(<code key={`${match.index}-c`}>{match[4]}</code>);
    else if (match[5]) nodes.push(<strong key={`${match.index}-s`}>{match[5]}</strong>);
    cursor = pattern.lastIndex;
  }
  if (cursor < text.length) nodes.push(text.slice(cursor));
  return nodes;
}

function CodeBlock({ code, language }: { code: string; language: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return <div className="doc-code"><div><span>{language}</span><button onClick={copy}>{copied ? <Check size={14} /> : <Copy size={14} />}{copied ? "Copied" : "Copy"}</button></div><pre><code>{code}</code></pre></div>;
}

function Markdown({ source }: { source: string }) {
  const lines = source.replaceAll("\r\n", "\n").split("\n");
  const output: ReactNode[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) { index += 1; continue; }
    if (line.startsWith("```")) {
      const language = line.slice(3).trim() || "text";
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].startsWith("```")) code.push(lines[index++]);
      index += 1;
      output.push(<CodeBlock code={code.join("\n")} language={language} key={`code-${index}`} />);
      continue;
    }
    const heading = /^(#{1,4})\s+(.+)$/.exec(line);
    if (heading) {
      const Tag = `h${heading[1].length}` as "h1" | "h2" | "h3" | "h4";
      const text = heading[2].replace(/`/g, "");
      output.push(<Tag id={textSlug(text)} key={`h-${index}`}>{inline(text)}</Tag>);
      index += 1;
      continue;
    }
    if (line.startsWith("| ") && lines[index + 1]?.match(/^\|?\s*:?-+/)) {
      const rows: string[][] = [];
      while (index < lines.length && lines[index].trim().startsWith("|")) rows.push(lines[index++].split("|").slice(1, -1).map((cell) => cell.trim()));
      const [head, , ...body] = rows;
      output.push(<div className="table-wrap" key={`table-${index}`}><table><thead><tr>{head.map((cell, i) => <th key={i}>{inline(cell)}</th>)}</tr></thead><tbody>{body.map((row, r) => <tr key={r}>{row.map((cell, c) => <td key={c}>{inline(cell)}</td>)}</tr>)}</tbody></table></div>);
      continue;
    }
    if (/^[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^[-*]\s+/.test(lines[index])) items.push(lines[index++].replace(/^[-*]\s+/, ""));
      output.push(<ul key={`ul-${index}`}>{items.map((item, i) => <li key={i}>{inline(item)}</li>)}</ul>);
      continue;
    }
    if (/^\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\d+\.\s+/.test(lines[index])) items.push(lines[index++].replace(/^\d+\.\s+/, ""));
      output.push(<ol key={`ol-${index}`}>{items.map((item, i) => <li key={i}>{inline(item)}</li>)}</ol>);
      continue;
    }
    if (line.startsWith("> ")) { output.push(<blockquote key={`quote-${index}`}>{inline(line.slice(2))}</blockquote>); index += 1; continue; }
    if (/^---+$/.test(line.trim())) { output.push(<hr key={`hr-${index}`} />); index += 1; continue; }
    const paragraph = [line.trim()];
    index += 1;
    while (index < lines.length && lines[index].trim() && !/^(#{1,4})\s|^```|^[-*]\s+|^\d+\.\s+|^>\s|^\|/.test(lines[index])) paragraph.push(lines[index++].trim());
    output.push(<p key={`p-${index}`}>{inline(paragraph.join(" "))}</p>);
  }
  return <>{output}</>;
}

export function DocsShell({ markdown, title, group }: { markdown: string; title: string; group: string }) {
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [dark, setDark] = useState(false);
  const headings = useMemo(() => markdown.split("\n").flatMap((line) => {
    const match = /^(#{2,3})\s+(.+)$/.exec(line);
    return match ? [{ level: match[1].length, text: match[2].replace(/[`*_]/g, ""), id: textSlug(match[2].replace(/[`*_]/g, "")) }] : [];
  }), [markdown]);
  const results = allDocs.filter((doc) => `${doc.title} ${doc.description} ${doc.group}`.toLowerCase().includes(query.toLowerCase()));
  const currentIndex = allDocs.findIndex((doc) => routeFor(doc.slug) === pathname);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") { event.preventDefault(); setSearchOpen(true); }
      if (event.key === "Escape") { setSearchOpen(false); setMenuOpen(false); }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  return <div className={dark ? "site dark" : "site"}>
    <header className="topbar">
      <Link className="brand" href="/docs" aria-label="Rustic docs home"><span className="brand-mark"><span /></span><span>RUSTIC</span><span className="brand-division">DOCS</span></Link>
      <button className="search-trigger" onClick={() => setSearchOpen(true)}><Search size={17} /><span>Search documentation</span><kbd><Command size={12} /> K</kbd></button>
      <nav className="top-actions" aria-label="Site links"><button className="version-button">v0.1 <ChevronDown size={14} /></button><a href="https://github.com" aria-label="GitHub"><Github size={19} /></a><button className="icon-button" onClick={() => setDark((value) => !value)} aria-label="Toggle theme">{dark ? <Sun size={18} /> : <Moon size={18} />}</button><button className="menu-button" onClick={() => setMenuOpen(true)} aria-label="Open navigation"><Menu /></button></nav>
    </header>
    <div className="docs-shell article-shell">
      <aside className={menuOpen ? "sidebar open" : "sidebar"}><div className="sidebar-mobile-head"><span>Documentation</span><button onClick={() => setMenuOpen(false)} aria-label="Close navigation"><X /></button></div><div className="sidebar-content">{docGroups.map((section) => <section className="nav-section" key={section.label}><p>{section.label}</p>{section.docs.map((doc) => <Link className={routeFor(doc.slug) === pathname ? "active" : ""} href={routeFor(doc.slug)} key={doc.slug} onClick={() => setMenuOpen(false)}>{doc.title}</Link>)}</section>)}</div></aside>
      {menuOpen && <button className="sidebar-scrim" onClick={() => setMenuOpen(false)} aria-label="Close navigation" />}
      <main className="content doc-content"><div className="breadcrumbs"><Link href="/docs">DOCS</Link><ArrowRight size={13} /><span>{group.toUpperCase()}</span><ArrowRight size={13} /><span>{title.toUpperCase()}</span></div><article className="markdown"><Markdown source={markdown} /></article><nav className="page-pagination" aria-label="Documentation pages">{currentIndex > 0 ? <Link href={routeFor(allDocs[currentIndex - 1].slug)}><ArrowLeft size={16} /><span><small>PREVIOUS</small>{allDocs[currentIndex - 1].title}</span></Link> : <span />}{currentIndex < allDocs.length - 1 ? <Link className="next" href={routeFor(allDocs[currentIndex + 1].slug)}><span><small>NEXT</small>{allDocs[currentIndex + 1].title}</span><ArrowRight size={16} /></Link> : null}</nav></main>
      <aside className="on-page"><p>ON THIS PAGE</p>{headings.map((heading) => <a className={heading.level === 3 ? "nested" : ""} href={`#${heading.id}`} key={heading.id}>{heading.text}</a>)}</aside>
    </div>
    {searchOpen && <div className="search-overlay" onMouseDown={(event) => event.target === event.currentTarget && setSearchOpen(false)}><section className="search-dialog" role="dialog" aria-modal="true" aria-label="Search documentation">
      <header className="search-dialog-head"><div className="search-identity"><span className="search-glyph"><span /></span><div><strong>RUSTIC INDEX</strong><small>ENGINE REFERENCE / 0.1</small></div></div><button className="search-close" onClick={() => setSearchOpen(false)} aria-label="Close search"><X size={15} /><span>ESC</span></button></header>
      <div className="search-input"><span className="search-prompt">/</span><input autoFocus value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Find an API, concept, or guide" aria-label="Search Rustic documentation" /><Search size={22} /></div>
      <div className="search-results"><div className="search-results-head"><p>{query ? "MATCHING ENTRIES" : "DOCUMENT INDEX"}</p><span>{String(results.length).padStart(2, "0")} / {String(allDocs.length).padStart(2, "0")}</span></div>{results.map((doc, index) => <Link href={routeFor(doc.slug)} onClick={() => setSearchOpen(false)} key={doc.slug}><span className="result-number">{String(index + 1).padStart(2, "0")}</span><span className="result-copy"><span className="result-title">{doc.title}</span><small><b>{doc.group}</b><span>{doc.description}</span></small></span><span className="result-arrow"><ArrowRight size={17} /></span></Link>)}{results.length === 0 && <div className="no-results"><span>404</span><strong>Nothing in the index</strong><small>Try a system name like “entity”, “time”, or “transform”.</small></div>}</div>
      <footer className="search-footer"><span><i /> LIVE DOC INDEX</span><span>LOCAL CATALOG / NO AI GUESSWORK</span></footer>
    </section></div>}
  </div>;
}
