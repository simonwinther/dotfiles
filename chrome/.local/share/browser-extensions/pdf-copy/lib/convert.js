// Turns PDF pages into a document model and selections into LaTeX or Markdown.
import { analyzePage } from "./layout.js";
import { buildBlocks } from "./structure.js";

export { readPageData } from "./page-data.js";
export { serializeSelection } from "./serialize.js";

export function analyze(data) {
  const model = analyzePage(data);
  model.items = data.items;
  model.blocks = buildBlocks(model.lines, model.context);
  return model;
}
