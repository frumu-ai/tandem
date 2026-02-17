import { defineCollection } from "astro:content";
import { docsSchema } from "@astrojs/starlight/schema";

const docs = defineCollection({
  schema: docsSchema(),
});

export const collections = { docs };
