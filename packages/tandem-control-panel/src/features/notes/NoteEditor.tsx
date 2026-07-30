import { PanelCard } from "../../ui/index.tsx";
import type { Note } from "./NotesList";

export type NoteUpdate = {
  title?: string;
  content?: string;
  updatedAt: number;
};

export function NoteEditor({
  note,
  onUpdate,
  onFlush,
}: {
  note: Note;
  onUpdate: (noteId: string, update: NoteUpdate) => void;
  onFlush: () => void;
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
            onUpdate(note.id, {
              title: event.target.value,
              updatedAt: Date.now(),
            })
          }
          onBlur={onFlush}
        />
        <textarea
          className="flex-1 w-full min-h-0 bg-transparent border border-white/6 rounded-lg p-3 resize-none"
          placeholder="Start typing your note here..."
          value={note.content}
          onChange={(event) =>
            onUpdate(note.id, {
              content: event.target.value,
              updatedAt: Date.now(),
            })
          }
          onBlur={onFlush}
        />
      </div>
    </PanelCard>
  );
}
