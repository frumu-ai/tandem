import { useState, useEffect } from "react";
import { PanelCard, IconButton, Icon } from "../../ui/index.tsx";
import type { Note, loadNotes, saveNotes } from "./NotesList";

export function NoteEditor({
  note,
  onUpdate,
}: {
  note: Note;
  onUpdate: (note: Note) => void;
}) {
  const [title, setTitle] = useState(note.title);
  const [content, setContent] = useState(note.content);

  useEffect(() => {
    setTitle(note.title);
    setContent(note.content);
  }, [note]);

  const handleChange = (newTitle: string, newContent: string) => {
    const updated = {
      ...note,
      title: newTitle,
      content: newContent,
      updatedAt: Date.now(),
    };
    onUpdate(updated);
  };

  return (
    <PanelCard title={title || "Untitled Note"} fullHeight>
      <div className="flex-1 flex flex-col min-h-0 gap-4">
        <input
          type="text"
          className="w-full bg-transparent border border-white/6 rounded-lg p-3 text-lg font-semibold"
          value={title}
          placeholder="Note title"
          onChange={(e) => {
            setTitle(e.target.value);
            handleChange(e.target.value, content);
          }}
        />
        <textarea
          className="flex-1 w-full min-h-0 bg-transparent border border-white/6 rounded-lg p-3 resize-none"
          placeholder="Start typing your note here..."
          value={content}
          onChange={(e) => {
            setContent(e.target.value);
            handleChange(title, e.target.value);
          }}
        />
      </div>
    </PanelCard>
  );
}
