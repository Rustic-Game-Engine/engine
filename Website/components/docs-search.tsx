"use client";

import { ArrowRight, ChevronDown, Github, Menu, Moon, Search, Sun, X } from "lucide-react";
import Link from "next/link";
import { useEffect, useRef, useState } from "react";
import { allDocs, docGroups, routeFor } from "@/lib/docs-catalog";

export function DocsSearch() {
  const [query, setQuery] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [dark, setDark] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const results = allDocs.filter((doc) => `${doc.title} ${doc.description} ${doc.group}`.toLowerCase().includes(query.toLowerCase()));

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") { event.preventDefault(); inputRef.current?.focus(); }
      if (event.key === "Escape") { setQuery(""); inputRef.current?.focus(); setMenuOpen(false); }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  return <div className={dark ? "site dark" : "site"}>
    <header className="topbar">
      <Link className="brand" href="/docs" aria-label="Rustic docs home"><span className="brand-mark"><span /></span><span>RUSTIC</span><span className="brand-division">DOCS</span></Link>
      <div className="search-trigger search-trigger-active"><Search size={17} /><span>Search documentation</span></div>
      <nav className="top-actions" aria-label="Site links"><button className="version-button">v0.1 <ChevronDown size={14} /></button><a href="https://github.com" aria-label="GitHub"><Github size={19} /></a><button className="icon-button" onClick={() => setDark((value) => !value)} aria-label="Toggle theme">{dark ? <Sun size={18} /> : <Moon size={18} />}</button><button className="menu-button" onClick={() => setMenuOpen(true)} aria-label="Open navigation"><Menu /></button></nav>
    </header>
    <div className="docs-shell search-shell">
      <aside className={menuOpen ? "sidebar open" : "sidebar"}><div className="sidebar-mobile-head"><span>Documentation</span><button onClick={() => setMenuOpen(false)} aria-label="Close navigation"><X /></button></div><div className="sidebar-content">{docGroups.map((section) => <section className="nav-section" key={section.label}><p>{section.label}</p>{section.docs.map((doc) => <Link href={routeFor(doc.slug)} key={doc.slug} onClick={() => setMenuOpen(false)}>{doc.title}</Link>)}</section>)}</div></aside>
      {menuOpen && <button className="sidebar-scrim" onClick={() => setMenuOpen(false)} aria-label="Close navigation" />}
      <main className="content doc-content search-page">
        <div className="breadcrumbs"><Link href="/docs">DOCS</Link><ArrowRight size={13} /><span>SEARCH</span></div>
        <section className="search-hero" aria-labelledby="search-title">
          <div className="search-hero-copy"><p><span /> DOCUMENT INDEX</p><h1 id="search-title">Search documentation</h1><div className="search-lede">Find engine APIs, scene concepts, and practical guides across the Rustic reference.</div></div>
          <div className="search-field"><Search size={21} /><input ref={inputRef} value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search by API, concept, or guide…" aria-label="Search Rustic documentation" /></div>
        </section>
        <section className="search-index" aria-label="Documentation index">
          <div className="search-index-head"><div><span>INDEX</span><h2>{query ? "Matching documentation" : "Browse all documentation"}</h2></div><p><strong>{String(results.length).padStart(2, "0")}</strong> / {String(allDocs.length).padStart(2, "0")} ENTRIES</p></div>
          <div className="search-index-list">{results.map((doc, index) => <Link href={routeFor(doc.slug)} key={doc.slug}><span className="result-number">{String(index + 1).padStart(2, "0")}</span><span className="result-copy"><span className="result-title">{doc.title}</span><small><b>{doc.group}</b><span>{doc.description}</span></small></span><span className="result-arrow"><ArrowRight size={18} /></span></Link>)}{results.length === 0 && <div className="no-results"><span>404</span><strong>Nothing in the index</strong><small>Try a system name like “entity”, “time”, or “transform”.</small></div>}</div>
        </section>
        <footer className="search-page-footer"><span><i /> LIVE DOC INDEX</span><span>LOCAL CATALOG / NO AI GUESSWORK</span></footer>
      </main>
      <aside className="on-page search-on-page"><p>SEARCH TIPS</p><span>Try an API name</span><code>transforms</code><span>Try a system</span><code>input</code><span>Try a workflow</span><code>gameplay</code></aside>
    </div>
  </div>;
}
