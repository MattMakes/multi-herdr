"use strict";
{
  function clean(text, limit) {
    const collapsed = String(text || "").replace(/\s+/g, " ").trim();
    return collapsed.length > limit ? collapsed.slice(0, limit) : collapsed;
  }

  function frameDocument(element) {
    let inner = null;
    try {
      inner = element.contentDocument || null;
    } catch (error) {
      inner = null;
    }
    return inner;
  }

  function isFrameTag(tag) {
    return tag === "IFRAME" || tag === "FRAME";
  }

  function shadowActive(element) {
    return element.shadowRoot && element.shadowRoot.activeElement ? element.shadowRoot.activeElement : null;
  }

  function innerFrameActive(element) {
    const inner = frameDocument(element);
    return inner && inner.activeElement && inner.activeElement !== inner.body ? inner.activeElement : null;
  }

  function frameActive(element) {
    return isFrameTag(element.tagName) ? innerFrameActive(element) : null;
  }

  function deeperActive(element) {
    return shadowActive(element) || frameActive(element);
  }

  function deepActiveElement() {
    let active = document.activeElement;
    let deeper = active && deeperActive(active);
    while (deeper) {
      active = deeper;
      deeper = deeperActive(active);
    }
    return active || null;
  }

  const implicitInputRoles = {
    button: "button", submit: "button", reset: "button", image: "button",
    checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton",
    search: "searchbox", email: "textbox", tel: "textbox", text: "textbox",
    url: "textbox", password: "textbox",
  };

  function anchorRole(element) {
    return element.hasAttribute("href") ? "link" : "";
  }

  function inputRole(element) {
    return implicitInputRoles[element.type] || "";
  }

  const implicitRoleReaders = new Map([["a", anchorRole], ["input", inputRole]]);
  const implicitTagRoles = new Map([
    ["button", "button"], ["summary", "button"], ["textarea", "textbox"], ["select", "combobox"],
  ]);

  function implicitRole(element, tag) {
    const read = implicitRoleReaders.get(tag);
    return read ? read(element) : implicitTagRoles.get(tag) || "";
  }

  function textOf(element) {
    return element.innerText !== undefined ? element.innerText : element.textContent || "";
  }

  function idText(element, id) {
    const root = element.getRootNode();
    const found = root.getElementById ? root.getElementById(id) : element.ownerDocument.getElementById(id);
    return found ? textOf(found) : "";
  }

  function labelledName(element) {
    const ids = element.getAttribute("aria-labelledby");
    return ids ? clean(ids.split(/\s+/).map(idText.bind(null, element)).join(" "), 80) : "";
  }

  function labelText(label) {
    return textOf(label);
  }

  function labelsName(element) {
    const labels = element.labels;
    return labels && labels.length ? clean(Array.from(labels).map(labelText).join(" "), 80) : "";
  }

  const buttonInputTypes = new Set(["button", "submit", "reset", "image"]);

  function isButtonInput(element) {
    return buttonInputTypes.has(element.type);
  }

  function buttonInputName(element, tag) {
    const named = tag === "input" && isButtonInput(element);
    return named ? clean(element.value || element.getAttribute("alt"), 80) : "";
  }

  function controlName(element, tag) {
    return labelsName(element) || buttonInputName(element, tag);
  }

  function imageAltName(element) {
    const image = element.querySelector && element.querySelector("img[alt]");
    return image ? clean(image.getAttribute("alt"), 80) : "";
  }

  function contentName(element) {
    return clean(textOf(element), 80) || imageAltName(element);
  }

  function isFormControlTag(tag) {
    return tag === "input" || tag === "textarea" || tag === "select";
  }

  function ownName(element, tag) {
    return isFormControlTag(tag) ? controlName(element, tag) : contentName(element);
  }

  function accessibleName(element, tag) {
    const named = labelledName(element) || clean(element.getAttribute("aria-label"), 80);
    return named || ownName(element, tag) || clean(element.getAttribute("title"), 80);
  }

  function sameTagSiblings(node, parent) {
    function hasSameTag(child) {
      return child.tagName === node.tagName;
    }
    return parent && parent.children ? Array.from(parent.children).filter(hasSameTag) : [];
  }

  function pathStep(node) {
    const tag = node.tagName.toLowerCase();
    const same = sameTagSiblings(node, node.parentElement || node.parentNode);
    return same.length > 1 ? tag + ":nth-of-type(" + (same.indexOf(node) + 1) + ")" : tag;
  }

  function hostPrefix(node) {
    const root = node.getRootNode();
    return root && root.host ? [structuralPath(root.host) + " >>>"] : [];
  }

  function stepsAt(node) {
    return [...(node.parentElement ? [] : hostPrefix(node)), pathStep(node)];
  }

  function structuralPath(element) {
    const steps = [];
    let node = element;
    while (node && node.nodeType === 1) {
      steps.unshift(...stepsAt(node));
      node = node.parentElement;
    }
    return steps.join(" > ").replace(/>>> >/g, ">>>");
  }

  function testIdOf(element) {
    return element.getAttribute("data-testid") || element.getAttribute("data-test") || element.getAttribute("data-test-id");
  }

  function hasFieldValue(entry) {
    return Boolean(entry[1]);
  }

  function namedFields(element, tag) {
    return [
      ["test_id", testIdOf(element)],
      ["id", element.id],
      ["role", element.getAttribute("role") || implicitRole(element, tag)],
      ["name", accessibleName(element, tag)],
      ["href", element.getAttribute("href")],
      ["form_name", element.getAttribute("name")],
      ["placeholder", element.getAttribute("placeholder")],
    ].filter(hasFieldValue);
  }

  function inputTypeField(element, tag) {
    return tag === "input" ? [["input_type", element.type]] : [];
  }

  function textField(element) {
    const text = clean(textOf(element), 40);
    return text ? [["text", text]] : [];
  }

  function fingerprint(element, frame) {
    const tag = element.tagName.toLowerCase();
    const fields = [...namedFields(element, tag), ...inputTypeField(element, tag), ...textField(element)];
    return Object.fromEntries([...fields, ["path", frame.pathPrefix + structuralPath(element)]]);
  }

  function reachable(frame, x, y) {
    if (!frame.host) return true;
    const parentX = x + frame.contentLeft;
    const parentY = y + frame.contentTop;
    const root = frame.host.getRootNode();
    const hit = root.elementFromPoint ? root.elementFromPoint(parentX, parentY) : null;
    if (hit !== frame.host) return false;
    return reachable(frame.parent, parentX, parentY);
  }

  function hits(element, x, y) {
    const root = element.getRootNode();
    const finder = root.elementFromPoint ? root : element.ownerDocument;
    const hit = finder.elementFromPoint(x, y);
    if (!hit) return false;
    return hit === element || element.contains(hit);
  }

  function isPointerHiddenStyle(style) {
    return style.visibility === "hidden" || style.visibility === "collapse" || style.pointerEvents === "none";
  }

  function pointerHidden(element) {
    const view = element.ownerDocument.defaultView;
    const style = view ? view.getComputedStyle(element) : null;
    return Boolean(style) && isPointerHiddenStyle(style);
  }

  function clippedRect(rect, clip) {
    return {
      left: Math.max(rect.left, clip.left),
      right: Math.min(rect.right, clip.right),
      top: Math.max(rect.top, clip.top),
      bottom: Math.min(rect.bottom, clip.bottom),
    };
  }

  function isEmptyRect(rect) {
    return rect.right - rect.left < 1 || rect.bottom - rect.top < 1;
  }

  function candidatePoints(rect) {
    const insetX = Math.min(4, (rect.right - rect.left) / 2);
    const insetY = Math.min(4, (rect.bottom - rect.top) / 2);
    const center = [(rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2];
    return [center, [rect.left + insetX, rect.top + insetY], [rect.right - insetX, rect.bottom - insetY]];
  }

  function roundedCoordinate(value) {
    return Math.round(value * 100) / 100;
  }

  function pagePoint(point, frame) {
    return { x: roundedCoordinate(point[0] + frame.offsetX), y: roundedCoordinate(point[1] + frame.offsetY) };
  }

  function pointInRect(element, frame, rect) {
    const clipped = clippedRect(rect, frame.clip);
    function isHit(point) {
      return hits(element, point[0], point[1]) && reachable(frame, point[0], point[1]);
    }
    const point = (isEmptyRect(clipped) ? [] : candidatePoints(clipped)).find(isHit);
    return point ? pagePoint(point, frame) : null;
  }

  function firstRectPoint(element, frame) {
    let found = null;
    function tryRect(rect) {
      found = pointInRect(element, frame, rect);
      return found !== null;
    }
    Array.from(element.getClientRects()).some(tryRect);
    return found;
  }

  function visiblePoint(element, frame) {
    return pointerHidden(element) ? null : firstRectPoint(element, frame);
  }

  function isDisabled(element) {
    const matched = Boolean(element.matches && element.matches(":disabled"));
    return matched || element.getAttribute("aria-disabled") === "true";
  }

  const textInputTypes = new Set([
    "text", "email", "number", "search", "tel", "url", "password",
  ]);

  function isTextLike(element) {
    const tag = element.tagName;
    return tag === "TEXTAREA" || (tag === "INPUT" && textInputTypes.has(element.type));
  }

  const interactiveRoles = new Set([
    "button", "link", "checkbox", "radio", "switch", "tab", "menuitem",
    "menuitemcheckbox", "menuitemradio", "option", "treeitem", "combobox",
    "slider", "spinbutton", "textbox", "searchbox", "gridcell",
  ]);

  function roleKind(element) {
    const role = (element.getAttribute("role") || "").trim().toLowerCase();
    return role && interactiveRoles.has(role) && !isDisabled(element) ? "role" : null;
  }

  function isSameContextTarget(target) {
    return !target || target === "_self" || target === "_top" || target === "_parent";
  }

  function linkKind(element) {
    const target = (element.getAttribute("target") || "").trim().toLowerCase();
    return isSameContextTarget(target) ? "link" : null;
  }

  function anchorKind(element) {
    return element.hasAttribute("href") ? linkKind(element) : roleKind(element);
  }

  function buttonKind(element) {
    return isDisabled(element) ? null : "button";
  }

  function textareaKind(element) {
    return isDisabled(element) ? null : "textarea";
  }

  function enabledInputKind(element) {
    return isButtonInput(element) ? "button" : "input";
  }

  function inputKind(element) {
    const skipped = element.type === "hidden" || element.type === "file" || isDisabled(element);
    return skipped ? null : enabledInputKind(element);
  }

  function isFileInput(control) {
    return control.tagName === "INPUT" && control.type === "file";
  }

  function labelKind(element) {
    const control = element.control;
    const usable = control && !isDisabled(control) && !isFileInput(control);
    return usable ? "label" : null;
  }

  const tagKinds = new Map([
    ["a", anchorKind],
    ["button", buttonKind],
    ["input", inputKind],
    ["textarea", textareaKind],
    ["label", labelKind],
  ]);

  function kindByTag(element) {
    const read = tagKinds.get(element.tagName.toLowerCase());
    return read ? read(element) : roleKind(element);
  }

  function isEditedActive(element, active) {
    return element === active && isTextLike(element) && element.value !== "";
  }

  function classify(element, active) {
    return isEditedActive(element, active) ? null : kindByTag(element);
  }

  function linkUrl(element) {
    let url = null;
    try {
      url = new URL(element.getAttribute("href"), element.baseURI).href;
    } catch (error) {
      url = null;
    }
    return url;
  }

  function withLinkUrl(candidate, element) {
    const url = linkUrl(element);
    return url === null ? null : { ...candidate, url };
  }

  function finishedCandidate(candidate, element) {
    return candidate.kind === "link" ? withLinkUrl(candidate, element) : candidate;
  }

  function candidateAt(element, frame, kind) {
    const point = visiblePoint(element, frame);
    const tag = element.tagName.toLowerCase();
    const candidate = point ? { kind, tag, target: fingerprint(element, frame), point } : null;
    return candidate ? finishedCandidate(candidate, element) : null;
  }

  function candidateOf(element, frame, scan) {
    const kind = classify(element, scan.active);
    return kind ? candidateAt(element, frame, kind) : null;
  }

  function consider(element, frame, scan) {
    const candidate = candidateOf(element, frame, scan);
    scan.clickables.push(...(candidate ? [candidate] : []));
  }

  function contentOrigin(element) {
    const rect = element.getBoundingClientRect();
    const style = element.ownerDocument.defaultView.getComputedStyle(element);
    const left = rect.left + element.clientLeft + (parseFloat(style.paddingLeft) || 0);
    return { left, top: rect.top + element.clientTop + (parseFloat(style.paddingTop) || 0) };
  }

  function viewSize(element, inner) {
    const view = inner.defaultView;
    return view ? { width: view.innerWidth, height: view.innerHeight } : { width: element.clientWidth, height: element.clientHeight };
  }

  function childClip(clip, origin, size) {
    return {
      left: Math.max(0, clip.left - origin.left),
      top: Math.max(0, clip.top - origin.top),
      right: Math.min(size.width, clip.right - origin.left),
      bottom: Math.min(size.height, clip.bottom - origin.top),
    };
  }

  function frameOffsets(frame, origin) {
    return {
      contentLeft: origin.left,
      contentTop: origin.top,
      offsetX: frame.offsetX + origin.left,
      offsetY: frame.offsetY + origin.top,
    };
  }

  function buildChildFrame(element, frame, inner) {
    const origin = contentOrigin(element);
    const clip = childClip(frame.clip, origin, viewSize(element, inner));
    const pathPrefix = frame.pathPrefix + structuralPath(element) + " >>| ";
    return { doc: inner, host: element, parent: frame, ...frameOffsets(frame, origin), clip, pathPrefix };
  }

  function childFrame(element, frame) {
    const inner = frameDocument(element);
    return inner && inner.documentElement ? buildChildFrame(element, frame, inner) : null;
  }

  function assignedOf(element) {
    return element.assignedElements ? element.assignedElements({ flatten: true }) : [];
  }

  function slotChildren(element) {
    const assigned = assignedOf(element);
    return assigned.length ? assigned : Array.from(element.children);
  }

  function lightChildren(element) {
    return element.tagName === "SLOT" ? slotChildren(element) : Array.from(element.children);
  }

  function childrenToWalk(element) {
    return element.shadowRoot ? Array.from(element.shadowRoot.children) : lightChildren(element);
  }

  function walkFrame(element, frame, scan) {
    const inner = childFrame(element, frame);
    return inner ? walk(inner.doc.documentElement, inner, scan) : null;
  }

  function walkChildren(element, frame, scan) {
    function walkChild(child) {
      walk(child, frame, scan);
    }
    childrenToWalk(element).forEach(walkChild);
  }

  function isWalkable(element, scan) {
    return Boolean(element) && element.nodeType === 1 && !scan.visited.has(element);
  }

  function visit(element, frame, scan) {
    scan.visited.add(element);
    consider(element, frame, scan);
    const descend = isFrameTag(element.tagName) ? walkFrame : walkChildren;
    descend(element, frame, scan);
  }

  function walk(element, frame, scan) {
    return isWalkable(element, scan) ? visit(element, frame, scan) : null;
  }

  function topFrame() {
    return {
      doc: document,
      host: null,
      parent: null,
      contentLeft: 0,
      contentTop: 0,
      offsetX: 0,
      offsetY: 0,
      clip: { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight },
      pathPrefix: "",
    };
  }

  function isFocusedField(active) {
    const isRoot = active === document.body || active === document.documentElement;
    return Boolean(active) && !isRoot && !isFrameTag(active.tagName);
  }

  function editableLength(active) {
    return active.isContentEditable ? (active.textContent || "").length : 0;
  }

  function valueLength(active) {
    return typeof active.value === "string" ? active.value.length : editableLength(active);
  }

  function focusedInfo(active) {
    const tag = active.tagName.toLowerCase();
    const target = fingerprint(active, { pathPrefix: "" });
    return { tag, input_type: tag === "input" ? active.type : null, value_length: valueLength(active), target };
  }

  function focusedOf(active) {
    return isFocusedField(active) ? focusedInfo(active) : null;
  }

  function maxScroll(scroller, scrollSize, clientSize) {
    return scroller ? Math.max(0, Math.round(scroller[scrollSize] - scroller[clientSize])) : 0;
  }

  function scrollInfo() {
    const scroller = document.scrollingElement || document.documentElement;
    const max_x = maxScroll(scroller, "scrollWidth", "clientWidth");
    const max_y = maxScroll(scroller, "scrollHeight", "clientHeight");
    return { x: Math.round(window.scrollX), y: Math.round(window.scrollY), max_x, max_y };
  }

  function contentInfo(contentType) {
    const is_html = contentType === "text/html" || contentType === "application/xhtml+xml";
    return { is_html, has_body: !!document.body };
  }

  function pageState(active, clickables) {
    const focused = focusedOf(active);
    const mime_type = document.contentType || "";
    const title = document.title || "";
    const scroll = scrollInfo();
    return { url: location.href, title, mime_type, scroll, content: contentInfo(mime_type), focused, clickables };
  }

  function captureState() {
    const active = deepActiveElement();
    const scan = { active, clickables: [], visited: new Set() };
    walk(document.documentElement, topFrame(), scan);
    return pageState(active, scan.clickables);
  }

  captureState();
}
