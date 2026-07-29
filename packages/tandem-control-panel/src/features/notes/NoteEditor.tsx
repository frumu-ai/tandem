import { PanelCard } from "../../ui/index.tsx";
import type { Note } from "./NotesList";

export function NoteEditor({
  note,
  onUpdate,
}: {
  note: Note;
  onUpdate: (note: Note) => void;
}) {
  return (
    <PanelCard title={note.title || "Untitled Note"} fullHeight>
      <div className="flex-1 flex flex-col min-h-0 gap-4">
        <input
          type="text"
          className="w-full bg-transparent border border-white/6 rounded-lg p-3 text-lg font-semibold"
          value={note.title}
          placeholder="Note title"
          onChange={(event) =>
            onUpdate({
              ...note,
              title: event.target.value,
              updatedAt: Date.now(),
            })
          }
        />
        <textarea
          className="flex-1 w-full min-h-0 bg-transparent border border-white/6 rounded-lg p-3 resize-none"
          placeholder="Start typing your note here..."
          value={note.content}
          onChange={(event) =>
            onUpdate({
              ...note,
              content: event.target.value,
              updatedAt: Date.now(),
            })
          }
        />
      </div>
    </PanelCard>
  );
}
