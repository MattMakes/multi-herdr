(() => {
  const interactiveRoles = new Set([
    "button", "link", "checkbox", "radio", "switch", "tab", "menuitem",
    "menuitemcheckbox", "menuitemradio", "option", "treeitem", "combobox",
    "slider", "spinbutton", "textbox", "searchbox", "gridcell",
  ]);
  const buttonInputTypes = new Set(["button", "submit", "reset", "image"]);
  const textInputTypes = new Set([
    "text", "email", "number", "search", "tel", "url", "password",
  ]);
  const implicitInputRoles = {
    button: "button", submit: "button", reset: "button", image: "button",
    checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton",
    search: "searchbox", email: "textbox", tel: "textbox", text: "textbox",
    url: "textbox", password: "textbox",
  };

  function clean(text, limit) {
    const collapsed = String(text || "").replace(/\s+/g, " ").trim();
    return collapsed.length > limit ? collapsed.slice(0, limit) : collapsed;
  }

  function frameDocument(element) {
    try {
      return element.contentDocument || null;
    } catch (error) {
      return null;
    }
  }

  function deepActiveElement() {
    let active = document.activeElement;
    for (;;) {
      if (!active) return null;
      if (active.shadowRoot && active.shadowRoot.activeElement) {
        active = active.shadowRoot.activeElement;
        continue;
      }
      const tag = active.tagName;
      if (tag === "IFRAME" || tag === "FRAME") {
        const inner = frameDocument(active);
        if (inner && inner.activeElement && inner.activeElement !== inner.body) {
          active = inner.activeElement;
          continue;
        }
      }
      return active;
    }
  }

  function implicitRole(element, tag) {
    if (tag === "a" && element.hasAttribute("href")) return "link";
    if (tag === "button" || tag === "summary") return "button";
    if (tag === "textarea") return "textbox";
    if (tag === "select") return "combobox";
    if (tag === "input") return implicitInputRoles[element.type] || "";
    return "";
  }

  function textOf(element) {
    if (element.innerText !== undefined) return element.innerText;
    return element.textContent || "";
  }

  function accessibleName(element, tag) {
    const labelledBy = element.getAttribute("aria-labelledby");
    if (labelledBy) {
      const root = element.getRootNode();
      const parts = labelledBy.split(/\s+/).map((id) => {
        const found = root.getElementById ? root.getElementById(id) : element.ownerDocument.getElementById(id);
        return found ? textOf(found) : "";
      });
      const joined = clean(parts.join(" "), 80);
      if (joined) return joined;
    }
    const label = clean(element.getAttribute("aria-label"), 80);
    if (label) return label;
    if (tag === "input" || tag === "textarea" || tag === "select") {
      if (element.labels && element.labels.length) {
        const fromLabels = clean(Array.from(element.labels).map(textOf).join(" "), 80);
        if (fromLabels) return fromLabels;
      }
      if (tag === "input" && buttonInputTypes.has(element.type)) {
        const value = clean(element.value || element.getAttribute("alt"), 80);
        if (value) return value;
      }
    } else {
      const own = clean(textOf(element), 80);
      if (own) return own;
      const image = element.querySelector && element.querySelector("img[alt]");
      if (image) {
        const alt = clean(image.getAttribute("alt"), 80);
        if (alt) return alt;
      }
    }
    return clean(element.getAttribute("title"), 80);
  }

  function structuralPath(element) {
    const steps = [];
    let node = element;
    while (node && node.nodeType === 1) {
      const tag = node.tagName.toLowerCase();
      const parent = node.parentElement || node.parentNode;
      let step = tag;
      if (parent && parent.children) {
        const same = Array.from(parent.children).filter((child) => child.tagName === node.tagName);
        if (same.length > 1) step += ":nth-of-type(" + (same.indexOf(node) + 1) + ")";
      }
      steps.unshift(step);
      if (node.parentElement) {
        node = node.parentElement;
      } else {
        const root = node.getRootNode();
        if (root && root.host) {
          steps.unshift(structuralPath(root.host) + " >>>");
        }
        break;
      }
    }
    return steps.join(" > ").replace(/>>> >/g, ">>>");
  }

  function fingerprint(element, frame) {
    const tag = element.tagName.toLowerCase();
    const result = {};
    const testId = element.getAttribute("data-testid") || element.getAttribute("data-test") || element.getAttribute("data-test-id");
    if (testId) result.test_id = testId;
    if (element.id) result.id = element.id;
    const role = element.getAttribute("role") || implicitRole(element, tag);
    if (role) result.role = role;
    const name = accessibleName(element, tag);
    if (name) result.name = name;
    const href = element.getAttribute("href");
    if (href) result.href = href;
    const formName = element.getAttribute("name");
    if (formName) result.form_name = formName;
    const placeholder = element.getAttribute("placeholder");
    if (placeholder) result.placeholder = placeholder;
    if (tag === "input") result.input_type = element.type;
    const text = clean(textOf(element), 40);
    if (text) result.text = text;
    result.path = frame.pathPrefix + structuralPath(element);
    return result;
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

  function visiblePoint(element, frame) {
    const view = element.ownerDocument.defaultView;
    const style = view ? view.getComputedStyle(element) : null;
    if (style && (style.visibility === "hidden" || style.visibility === "collapse" || style.pointerEvents === "none")) return null;
    const rects = element.getClientRects();
    for (const rect of rects) {
      const left = Math.max(rect.left, frame.clip.left);
      const right = Math.min(rect.right, frame.clip.right);
      const top = Math.max(rect.top, frame.clip.top);
      const bottom = Math.min(rect.bottom, frame.clip.bottom);
      if (right - left < 1 || bottom - top < 1) continue;
      const candidates = [
        [(left + right) / 2, (top + bottom) / 2],
        [left + Math.min(4, (right - left) / 2), top + Math.min(4, (bottom - top) / 2)],
        [right - Math.min(4, (right - left) / 2), bottom - Math.min(4, (bottom - top) / 2)],
      ];
      for (const [x, y] of candidates) {
        if (hits(element, x, y) && reachable(frame, x, y)) {
          return { x: Math.round((x + frame.offsetX) * 100) / 100, y: Math.round((y + frame.offsetY) * 100) / 100 };
        }
      }
    }
    return null;
  }

  function isDisabled(element) {
    if (element.matches && element.matches(":disabled")) return true;
    return element.getAttribute("aria-disabled") === "true";
  }

  function isTextLike(element) {
    const tag = element.tagName;
    if (tag === "TEXTAREA") return true;
    return tag === "INPUT" && textInputTypes.has(element.type);
  }

  const active = deepActiveElement();
  const clickables = [];
  const visited = new Set();

  function classify(element) {
    const tag = element.tagName.toLowerCase();
    if (element === active && isTextLike(element) && element.value !== "") return null;
    if (tag === "a" && element.hasAttribute("href")) {
      const target = (element.getAttribute("target") || "").trim().toLowerCase();
      if (target && target !== "_self" && target !== "_top" && target !== "_parent") return null;
      return "link";
    }
    if (tag === "button") return isDisabled(element) ? null : "button";
    if (tag === "input") {
      if (element.type === "hidden" || element.type === "file" || isDisabled(element)) return null;
      return buttonInputTypes.has(element.type) ? "button" : "input";
    }
    if (tag === "textarea") return isDisabled(element) ? null : "textarea";
    if (tag === "label") {
      const control = element.control;
      if (!control || isDisabled(control)) return null;
      if (control.tagName === "INPUT" && control.type === "file") return null;
      return "label";
    }
    const role = (element.getAttribute("role") || "").trim().toLowerCase();
    if (role && interactiveRoles.has(role) && !isDisabled(element)) return "role";
    return null;
  }

  function consider(element, frame) {
    const kind = classify(element);
    if (!kind) return;
    const point = visiblePoint(element, frame);
    if (!point) return;
    const candidate = {
      kind,
      tag: element.tagName.toLowerCase(),
      target: fingerprint(element, frame),
      point,
    };
    if (kind === "link") {
      try {
        candidate.url = new URL(element.getAttribute("href"), element.baseURI).href;
      } catch (error) {
        return;
      }
    }
    clickables.push(candidate);
  }

  function childFrame(element, frame) {
    const inner = frameDocument(element);
    if (!inner || !inner.documentElement) return null;
    const view = inner.defaultView;
    const rect = element.getBoundingClientRect();
    const style = element.ownerDocument.defaultView.getComputedStyle(element);
    const contentLeft = rect.left + element.clientLeft + (parseFloat(style.paddingLeft) || 0);
    const contentTop = rect.top + element.clientTop + (parseFloat(style.paddingTop) || 0);
    const width = view ? view.innerWidth : element.clientWidth;
    const height = view ? view.innerHeight : element.clientHeight;
    return {
      doc: inner,
      host: element,
      parent: frame,
      contentLeft,
      contentTop,
      offsetX: frame.offsetX + contentLeft,
      offsetY: frame.offsetY + contentTop,
      clip: {
        left: Math.max(0, frame.clip.left - contentLeft),
        top: Math.max(0, frame.clip.top - contentTop),
        right: Math.min(width, frame.clip.right - contentLeft),
        bottom: Math.min(height, frame.clip.bottom - contentTop),
      },
      pathPrefix: frame.pathPrefix + structuralPath(element) + " >>| ",
    };
  }

  function walk(element, frame) {
    if (!element || element.nodeType !== 1 || visited.has(element)) return;
    visited.add(element);
    consider(element, frame);
    const tag = element.tagName;
    if (tag === "IFRAME" || tag === "FRAME") {
      const inner = childFrame(element, frame);
      if (inner) walk(inner.doc.documentElement, inner);
      return;
    }
    if (element.shadowRoot) {
      for (const child of Array.from(element.shadowRoot.children)) walk(child, frame);
      return;
    }
    if (tag === "SLOT") {
      const assigned = element.assignedElements ? element.assignedElements({ flatten: true }) : [];
      if (assigned.length) {
        for (const child of assigned) walk(child, frame);
        return;
      }
    }
    for (const child of Array.from(element.children)) walk(child, frame);
  }

  const topFrame = {
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
  if (document.documentElement) walk(document.documentElement, topFrame);

  let focused = null;
  if (active && active !== document.body && active !== document.documentElement && active.tagName !== "IFRAME" && active.tagName !== "FRAME") {
    const ownerFrame = { pathPrefix: "" };
    const tag = active.tagName.toLowerCase();
    let valueLength = 0;
    if (typeof active.value === "string") valueLength = active.value.length;
    else if (active.isContentEditable) valueLength = (active.textContent || "").length;
    focused = {
      tag,
      input_type: tag === "input" ? active.type : null,
      value_length: valueLength,
      target: fingerprint(active, ownerFrame),
    };
  }

  const scroller = document.scrollingElement || document.documentElement;
  const contentType = document.contentType || "";
  return {
    url: location.href,
    title: document.title || "",
    mime_type: contentType,
    scroll: {
      x: Math.round(window.scrollX),
      y: Math.round(window.scrollY),
      max_x: scroller ? Math.max(0, Math.round(scroller.scrollWidth - scroller.clientWidth)) : 0,
      max_y: scroller ? Math.max(0, Math.round(scroller.scrollHeight - scroller.clientHeight)) : 0,
    },
    content: {
      is_html: contentType === "text/html" || contentType === "application/xhtml+xml",
      has_body: !!document.body,
    },
    focused,
    clickables,
  };
})()
