import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { DocsShell } from "@/components/docs-shell";
import { allDocs } from "@/lib/docs-catalog";
import { findDoc, readDoc } from "@/lib/docs";

export const dynamicParams = false;

export function generateStaticParams() {
  return allDocs.map((doc) => ({ slug: doc.slug ? doc.slug.split("/") : undefined }));
}

export async function generateMetadata({ params }: PageProps<"/docs/[[...slug]]">): Promise<Metadata> {
  const { slug } = await params;
  const doc = findDoc(slug);
  return doc ? { title: `${doc.title} · Rustic Engine Docs`, description: doc.description } : {};
}

export default async function DocumentationPage({ params }: PageProps<"/docs/[[...slug]]">) {
  const { slug } = await params;
  const doc = findDoc(slug);
  if (!doc) notFound();
  return <DocsShell markdown={await readDoc(doc.source)} title={doc.title} group={doc.group} />;
}
