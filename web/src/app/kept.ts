/** A package kept in the project (GET /v1/kept): the version it gives each node. */
export interface Kept {
  id: string;
  versions: Record<string, string>;
  bytes: number;
}

/** The write sheet's version choices: a package giving every node one version is named by it, others by their id. */
export function keptChoices(list: Kept[]): { id: string; label: string }[] {
  return list.map((k) => {
    const one = [...new Set(Object.values(k.versions))];
    return { id: k.id, label: one.length === 1 ? one[0] : k.id };
  });
}
