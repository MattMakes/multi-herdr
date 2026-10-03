# GSAP in Vue, Nuxt, Svelte and other frameworks

For React and Next.js, see `react.md`.

## The same 3 rules everywhere

1. Create animations after the component's DOM exists (the mount hook).
2. Create them inside `gsap.context(fn, root)` so selector text is scoped to the component root and everything is recorded.
3. Call `ctx.revert()` in the unmount hook. This kills tweens, timelines and ScrollTriggers and restores inline styles.

Register plugins once at app level (an entry file or a framework plugin), not in each component.

| Lifecycle | Action |
|---|---|
| Mount | `ctx = gsap.context(() => { ... }, root)` |
| DOM update after data loads | `ScrollTrigger.refresh()` after the framework's next-tick |
| Unmount | `ctx.revert()` |

## Vue 3 (`<script setup>`)

```vue
<script setup>
import { ref, onMounted, onUnmounted } from "vue";
import { gsap } from "gsap";

const root = ref(null);
let ctx;

onMounted(() => {
  ctx = gsap.context(() => {
    gsap.from(".item", { autoAlpha: 0, y: 20, stagger: 0.1 });
    gsap.timeline({ scrollTrigger: { trigger: ".band", start: "top center", end: "bottom center", scrub: true } })
      .to(".band-box", { x: 200, rotation: 360 });
  }, root.value);
});

onUnmounted(() => ctx?.revert());
</script>

<template>
  <div ref="root">
    <div class="item">One</div>
    <div class="item">Two</div>
    <section class="band"><div class="band-box" /></section>
  </div>
</template>
```

- After async data renders new elements, `await nextTick()` and then create their animations with `ctx.add(() => { ... })`, and call `ScrollTrigger.refresh()`.
- `ctx.add()` also wraps event-handler animations so `ctx.revert()` cleans them up.
- Wrap setup in `gsap.matchMedia()` instead of `gsap.context()` when you need breakpoints or reduced motion; call `mm.revert()` on unmount.

## Nuxt

GSAP must run only on the client. `onMounted` runs only on the client, so the Vue pattern above is SSR-safe.

Register plugins once in a client plugin, or in a composable that the components call. A composable can also lazy-load rarely used plugins:

```typescript
// composables/useGsap.ts
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

const loaders = {
  SplitText: () => import("gsap/SplitText"),
  Flip: () => import("gsap/Flip"),
  MorphSVGPlugin: () => import("gsap/MorphSVGPlugin"),
} as const;

export function useGsap() {
  gsap.registerPlugin(ScrollTrigger);

  async function loadPlugin<K extends keyof typeof loaders>(name: K) {
    const mod: any = await loaders[name]();
    const plugin = mod[name];
    gsap.registerPlugin(plugin);
    return plugin;
  }

  return { gsap, ScrollTrigger, loadPlugin };
}
```

```vue
<script setup>
const { gsap, loadPlugin } = useGsap();
const root = ref(null);              // <main ref="root"> in the template
let ctx;

onMounted(async () => {
  const SplitText = await loadPlugin("SplitText");
  ctx = gsap.context(() => {
    const split = SplitText.create("h1", { type: "chars" });
    gsap.from(split.chars, { autoAlpha: 0, y: -40, stagger: { amount: 0.3 } });
  }, root.value);
});

onUnmounted(() => ctx?.revert());      // unmount, not a second onMounted
</script>
```

Do not name a composable `useGSAP`: that name belongs to the React hook and confuses readers and search.

## Svelte and SvelteKit

`onMount` runs only in the browser, and its returned function runs on destroy.

```svelte
<script>
  import { onMount } from "svelte";
  import { gsap } from "gsap";

  let root;

  onMount(() => {
    const ctx = gsap.context(() => {
      gsap.from(".item", { autoAlpha: 0, y: 20, stagger: 0.1 });
    }, root);
    return () => ctx.revert();
  });
</script>

<div bind:this={root}>
  <div class="item">One</div>
  <div class="item">Two</div>
</div>
```

- Svelte 5: `$effect(() => { const ctx = gsap.context(..., root); return () => ctx.revert(); })` follows the same rule; `onMount` still works.
- After state changes add elements, `await tick()` before you animate them, then `ScrollTrigger.refresh()`.
- Svelte's own `transition:` directives are fine for simple enter and leave. Use GSAP for sequences, scroll and plugins.

## Astro, Angular, web components, vanilla multi-page

- Astro: run GSAP in a `<script>` in the component (it runs on the client). With view transitions, create on `astro:page-load` and revert on `astro:before-swap`.
- Angular: create in `ngAfterViewInit` with the host element as scope (`gsap.context(fn, this.el.nativeElement)`), revert in `ngOnDestroy`. Run it outside the Angular zone (`NgZone.runOutsideAngular`) to avoid change detection on every frame.
- Web components: create in `connectedCallback`, revert in `disconnectedCallback`; scope to the shadow root.
- Vanilla pages: wait for `DOMContentLoaded` (or use `type="module"` scripts, which are deferred), and for SPA-like page swaps keep one context per page and revert it before the swap.

## Common mistakes

- Animating in `setup` or top-level script before the DOM exists.
- Selector text without a context scope: it can animate other component instances.
- No `ctx.revert()` on unmount.
- A cleanup registered in a second mount hook instead of the unmount hook.
- Registering plugins in every component.
