// Tests only: two happy-dom gaps that make DOMPurify keep what a browser's DOMPurify strips.
//
// 1. A browser defines `nodeName` once, on `Node.prototype`, and that getter answers for every node. DOMPurify calls
//    it directly. happy-dom defines it again on each node class and its base getter answers "", so DOMPurify read
//    every tag name as "". The base getter here dispatches to the node's own class.
// 2. A browser's NodeIterator survives the removal of the node it stands on: it steps back to the node before it
//    (DOM, "NodeIterator pre-removing steps"). happy-dom's walks on from the detached node and stops, so DOMPurify
//    never visited the nodes after a removed `<script>` and kept their event handlers. This iterator steps back to
//    the last node it gave that is still under the root, which is the same node for DOMPurify's walk.
const base = Object.getOwnPropertyDescriptor(Node.prototype, "nodeName")!;
Object.defineProperty(Node.prototype, "nodeName", {
  configurable: true,
  get(this: Node) {
    for (let p = Object.getPrototypeOf(this); p && p !== Node.prototype; p = Object.getPrototypeOf(p)) {
      const own = Object.getOwnPropertyDescriptor(p, "nodeName");
      if (own?.get) return own.get.call(this);
    }
    return base.get!.call(this);
  },
});

function after(node: Node, root: Node): Node | null {
  if (node.firstChild) return node.firstChild;
  for (let n: Node | null = node; n && n !== root; n = n.parentNode) if (n.nextSibling) return n.nextSibling;
  return null;
}

// window.Document is a per-window subclass; the method lives on happy-dom's own Document class above it.
let owner = document as object;
while (!Object.hasOwn(owner, "createNodeIterator")) owner = Object.getPrototypeOf(owner);
(owner as Document).createNodeIterator = function (root: Node, whatToShow = 0xffffffff, filter?: NodeFilter | null) {
  const given: Node[] = [];
  const accept = (n: Node) =>
    (whatToShow & (1 << (n.nodeType - 1))) !== 0 &&
    (!filter || (typeof filter === "function" ? filter(n) : filter.acceptNode(n)) === NodeFilter.FILTER_ACCEPT);
  return {
    root,
    whatToShow,
    filter: filter ?? null,
    nextNode() {
      while (given.length && !(given.at(-1) === root || root.contains(given.at(-1)!))) given.pop();
      for (let n = given.length ? after(given.at(-1)!, root) : root; n; n = after(n, root)) {
        if (accept(n)) return (given.push(n), n);
      }
      return null;
    },
  } as unknown as NodeIterator;
};
