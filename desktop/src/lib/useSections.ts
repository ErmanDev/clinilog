import { useCallback, useEffect, useState } from "react";
import { asCmdError, sectionsApi, type Section } from "./api";

/** Loads the school's sections; `reload` refreshes after a change. */
export function useSections() {
  const [sections, setSections] = useState<Section[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setSections(await sectionsApi.list());
      setError(null);
    } catch (e) {
      setError(asCmdError(e).message);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { sections, error, reload };
}

/** Sections grouped by grade, in grade order, for <optgroup>s. */
export function groupByGrade(sections: Section[]): [number, Section[]][] {
  const map = new Map<number, Section[]>();
  for (const s of sections) {
    const list = map.get(s.grade_level) ?? [];
    list.push(s);
    map.set(s.grade_level, list);
  }
  return [...map.entries()].sort((a, b) => a[0] - b[0]);
}
