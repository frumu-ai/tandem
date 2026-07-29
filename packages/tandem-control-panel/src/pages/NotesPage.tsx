import { useState, useEffect } from "react";
import { AnimatedPage, SplitView, EmptyState } from "../ui/index.tsx";
import { NotesList, loadNotes, saveNotes, type Note } from "../features/notes/NotesList";
import { NoteEditor } from "../features/notes/NoteEditor";
import type { AppPageProps } from "./pageTypes";

export function NotesPage({}: AppPageProps) {
  const [notes, setNotes] = useState<Note[]>(() => loadNotes());
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const selectedNote = notes.find((n) => n.id === selectedNoteId) || null;

  const updateNote = (updatedNote: Note) => {
    const next = notes.map((n) => (n.id === updatedNote.id ? updatedNote : n));
    setNotes(next);
    saveNotes(next);
  };

  return (
    <AnimatedPage className="h-full min-h-0">
      <SplitView
        className="h-full min-h-0"
        mainClassName="h-full min-h-0 flex"
        asideClassName="h-full min-h-0 flex"
        main={
          selectedNote ? (
            <NoteEditor note={selectedNote} onUpdate={updateNote} />
          ) : (
            <div className="flex-1">
              <EmptyState
                title="Select a note"
                text="Select a note from the sidebar or create a new one to get started"
              />
            </div>
          )
        }
        aside={
          <NotesList
            onSelectNote={(note) => setSelectedNoteId(note.id)}
          />
        }
      />
    </AnimatedPage>
  );
}
