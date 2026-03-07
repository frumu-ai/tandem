import { writeKnowledgeBundle } from "./engine-knowledge-bundle.mjs";

writeKnowledgeBundle()
  .then(({ manifest }) => {
    process.stdout.write(
      `generated bundle: files=${manifest.file_count} bytes=${manifest.total_bytes} hash=${manifest.corpus_hash}\n`
    );
  })
  .catch((err) => {
    process.stderr.write(`${err?.stack || String(err)}\n`);
    process.exit(1);
  });
