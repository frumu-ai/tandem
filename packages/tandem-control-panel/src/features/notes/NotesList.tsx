import { useState, useEffect } from "react";
import { PanelCard, EmptyState, IconButton, Icon } from "../../ui/index.tsx";

type Note = {
  id: string;
  title: string;
  content: string;
  createdAt: number;
  updatedAt: number;
};

const STORAGE_KEY = "tandem-control-panel-notes";

function loadNotes(): Note[] {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (!stored) return [];
    const parsed = JSON.parse(stored);
    if (!Array.isArray(parsed)) return [];
    return parsed;
  } catch {
    return [];
  }
}

function saveNotes(notes: Note[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(notes));
  } catch {
    // ignore
  }
}

export function NotesList({ onSelectNote }: { onSelectNote: (note: Note) => void }) {
  const [notes, setNotes] = useState<Note[]>(() => loadNotes());

  const createNote = () => {
    const newNote: Note = {
      id: crypto.randomUUID(),
      title: "New Note",
      content: "",
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    const next = [newNote, ...notes];
    setNotes(next);
    saveNotes(next);
    onSelectNote(newNote);
  };

  const deleteNote = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    const next = notes.filter((n) => n.id !== id);
    setNotes(next);
    saveNotes(next);
  };

  return (
    <PanelCard
      title="Notes"
      actions={
        <button type="button" className="tcp-btn-primary" onClick={createNote}>
          <Icon name="plus" />
          New Note
        </button>
      }
      fullHeight
    >
      <div className="flex-1 overflow-auto min-h-0">
        {notes.length === 0 ? (
          <EmptyState
            text="Create your first note to get started"
            action={
              <button type="button" className="tcp-btn-primary" onClick={createNote}>
                Create Note
              </button>
            }
          />
        ) : (
          <div className="grid gap-2">
            {notes.map((note) => (
              <div
                key={note.id}
                className="flex items-center justify-between gap-3 p-3 rounded-lg border border-white/6 bg-white/3 hover:bg-white/6 cursor-pointer"
                onClick={() => onSelectNote(note)}
              >
                <div className="min-w-0">
                  <div className="font-semibold truncate">
                    {note.title || "Untitled Note"}
                  </div>
                  <div className="text-xs tcp-subtle mt-0.5 truncate">
                    {note.content || "No content"}
                  </div>
                </div>
                <IconButton
                  aria-label="Delete note"
                  onClick={(e) => deleteNote(note.id, e)}
                >
                  <Icon name="trash-2" />
                </IconButton>
              </div>
            ))}
          </div>
        )}
      </div>
    </PanelCard>
  );
}

export { loadNotes, saveNotes, type Note };
