import { useEffect, useState } from "react";
import { AnimatedPage, SplitView, EmptyState } from "../ui/index.tsx";
import {
  NotesList,
  loadNotes,
  saveNotes,
  type Note,
} from "../features/notes/NotesList";
import { NoteEditor } from "../features/notes/NoteEditor";
import type { AppPageProps } from "./pageTypes";

const NOTES_LOAD_ERROR = "Notes could not be loaded for this account.";
const NOTES_SAVE_ERROR =
  "Notes could not be saved in this browser. Check storage permissions or available space.";

export function NotesPage({ principalId, toast }: AppPageProps) {
  const [notes, setNotes] = useState<Note[]>([]);
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [storageError, setStorageError] = useState<string | null>(null);
  const selectedNote = notes.find((note) => note.id === selectedNoteId) || null;

  useEffect(() => {
    setSelectedNoteId(null);
    try {
      setNotes(loadNotes(principalId));
      setStorageError(null);
    } catch {
      setNotes([]);
      setStorageError(NOTES_LOAD_ERROR);
    }
  }, [principalId]);

  const persistNotes = (nextNotes: Note[]): boolean => {
    try {
      saveNotes(principalId, nextNotes);
      setNotes(nextNotes);
      setStorageError(null);
      return true;
    } catch {
      setStorageError(NOTES_SAVE_ERROR);
      toast("err", NOTES_SAVE_ERROR);
      return false;
    }
  };

  const createNote = () => {
    const now = Date.now();
    const note: Note = {
      id: crypto.randomUUID(),
      title: "New Note",
      content: "",
      createdAt: now,
      updatedAt: now,
    };
    if (persistNotes([note, ...notes])) {
      setSelectedNoteId(note.id);
    }
  };

  const updateNote = (updatedNote: Note) => {
    persistNotes(notes.map((note) => (note.id === updatedNote.id ? updatedNote : note)));
  };

  const deleteNote = (noteId: string) => {
    if (persistNotes(notes.filter((note) => note.id !== noteId)) && selectedNoteId === noteId) {
      setSelectedNoteId(null);
    }
  };

  return (
    <AnimatedPage className="h-full min-h-0 flex flex-col gap-3">
      {storageError ? (
        <div
          role="alert"
          className="rounded-lg border border-red-400/30 bg-red-950/30 px-3 py-2 text-sm text-red-100"
        >
          {storageError}
        </div>
      ) : null}
      <SplitView
        className="flex-1 min-h-0"
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
            notes={notes}
            selectedNoteId={selectedNoteId}
            onCreateNote={createNote}
            onSelectNote={setSelectedNoteId}
            onDeleteNote={deleteNote}
          />
        }
      />
    </AnimatedPage>
  );
}
