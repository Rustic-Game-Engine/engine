import type { Metadata } from "next";
import { DocsSearch } from "@/components/docs-search";

export const metadata: Metadata = {
  title: "Search · Rustic Engine Docs",
  description: "Search the Rustic Engine documentation index.",
};

export default function SearchPage() {
  return <DocsSearch />;
}
