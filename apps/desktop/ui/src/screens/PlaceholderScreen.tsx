interface Props {
  title: string;
  note?: string;
}

export function PlaceholderScreen({ title, note }: Props) {
  return (
    <section>
      <h1 className="text-lg font-semibold">{title}</h1>
      <p className="mt-4 max-w-xl text-sm text-zinc-500 dark:text-zinc-400">
        {note ?? "Not implemented yet."}
      </p>
    </section>
  );
}
