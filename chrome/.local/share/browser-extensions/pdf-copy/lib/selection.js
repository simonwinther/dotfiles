// Maps a DOM selection in PDF.js text layers to text item character ranges.

// Character offset of a DOM boundary point within a text layer span.
function offsetWithin(span, node, offset) {
  if (node === span || (node.nodeType !== Node.TEXT_NODE && span.contains(node))) {
    const range = document.createRange();
    range.setStart(span, 0);
    range.setEnd(node, offset);
    return range.toString().length;
  }
  let total = 0;
  const walker = document.createTreeWalker(span, NodeFilter.SHOW_TEXT);
  for (let current = walker.nextNode(); current; current = walker.nextNode()) {
    if (current === node) return total + offset;
    total += current.length;
  }
  return total;
}

// Returns [{ pageNumber, leaves: Map<leafIndex, [start, end][]>, spans }] for selected text layers.
export function selectedLeaves(selection, root) {
  const pages = new Map();
  for (let index = 0; index < selection.rangeCount; index++) {
    const range = selection.getRangeAt(index);
    for (const layer of root.querySelectorAll(".page .textLayer")) {
      if (!range.intersectsNode(layer)) continue;
      const pageNumber = Number(layer.closest(".page")?.dataset.pageNumber);
      if (!pageNumber) continue;
      const spans = [...layer.querySelectorAll('span[role="presentation"]')];
      const entry = pages.get(pageNumber) || { pageNumber, spans, leaves: new Map() };
      pages.set(pageNumber, entry);
      spans.forEach((span, leaf) => {
        if (!range.intersectsNode(span)) return;
        const length = span.textContent.length;
        const start = span.contains(range.startContainer) ? offsetWithin(span, range.startContainer, range.startOffset) : 0;
        const end = span.contains(range.endContainer) ? offsetWithin(span, range.endContainer, range.endOffset) : length;
        if (end <= start) return;
        const list = entry.leaves.get(leaf) || [];
        list.push([start, end]);
        entry.leaves.set(leaf, list);
      });
    }
  }
  return [...pages.values()].filter((page) => page.leaves.size).sort((a, b) => a.pageNumber - b.pageNumber);
}

// The text layer renders one span per non-empty text item, in order.
export function itemSelection(page, items) {
  const nonEmpty = [];
  items.forEach((item, index) => {
    if (item.str) nonEmpty.push(index);
  });
  if (nonEmpty.length !== page.spans.length) return null;
  const selection = new Map();
  for (const [leaf, ranges] of page.leaves) {
    const itemIndex = nonEmpty[leaf];
    if (items[itemIndex].str !== page.spans[leaf].textContent) return null;
    selection.set(itemIndex, ranges);
  }
  return selection;
}
