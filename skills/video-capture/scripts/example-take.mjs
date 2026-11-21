export const config = { dsf: 1, fixtures: { 'https://api.example.test/items': { json: { count: 3 }, delay_ms: 900 } } };
export default async (h) => {
  await h.goto(new URL('../../app/index.html', import.meta.url).href);
  await h.hold(300);
  await h.click('#q');
  await h.zoomTo('#q', 1.6);
  await h.type('flaky tests');
  await h.click('#go');
  await h.wait('document.querySelector("#res").textContent !== ""');
  h.zoom(1);
  await h.hold(500);
};
