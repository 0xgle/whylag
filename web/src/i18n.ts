export type Lang = "en" | "pl";

export const STRINGS = {
  en: {
    title: "Why is my internet lagging?",
    lead: "A 30-second test that measures your speed and the lag your calls and games feel when the line is busy. Then it tells you what to fix.",
    start: "Start the test",
    again: "Test again",
    consent: "The test uses M-Lab's open measurement servers. M-Lab publishes every result, including your IP address, as open research data.",
    phaseLocate: "Finding the nearest test server",
    phaseDownload: "Measuring download",
    phaseUpload: "Measuring upload",
    phaseDone: "Done",
    download: "Download",
    upload: "Upload",
    lag: "Lag under load",
    idle: "idle",
    grade: "Your grade",
    share: "Share result",
    saved: "Image saved",
    server: "Test server: {server}",
    failed: "The test could not reach M-Lab. Check your connection or try again in a minute.",
    verdictGood: "Your line is in good shape",
    verdictBloat: "Your line is fast, but it lags whenever someone downloads",
    verdictSlow: "Your connection is too slow for comfortable streaming and calls",
    verdictBoth: "Your connection is slow and lags under load",
    tipBloat: "Lag jumps from {idle} ms to {loaded} ms under load. Turn on SQM, Smart Queue or Adaptive QoS in your router and set it to about 90% of your line speed.",
    tipSlow: "Measured {down} Mb/s. Check your plan and test again with a cable to rule out Wi-Fi.",
    tipWifi: "If things still stutter, the culprit is usually Wi-Fi: weak signal, a crowded channel or a router in the wrong spot. A browser cannot see that, but WhyLag can.",
    appTitle: "Find out exactly where it breaks",
    appBody: "WhyLag for Windows, macOS and Linux checks your Wi-Fi, maps coverage room by room, finds every device on your network, follows your traffic hop by hop and tells you whether to blame your router or your provider.",
    appCta: "Download WhyLag",
    appFree: "Free download",
    footer: "Speed measured with M-Lab (measurementlab.net).",
    shareText: "My internet got a {grade} on WhyLag. How does yours do?",
  },
  pl: {
    title: "Czemu mój internet laguje?",
    lead: "Test w 30 sekund: mierzy prędkość i lagi, które czujesz w rozmowach i grach, gdy łącze jest zajęte. A potem mówi, co naprawić.",
    start: "Rozpocznij test",
    again: "Testuj ponownie",
    consent: "Test korzysta z otwartych serwerów pomiarowych M-Lab. M-Lab publikuje każdy wynik, łącznie z Twoim adresem IP, jako otwarte dane badawcze.",
    phaseLocate: "Szukam najbliższego serwera",
    phaseDownload: "Mierzę pobieranie",
    phaseUpload: "Mierzę wysyłanie",
    phaseDone: "Gotowe",
    download: "Pobieranie",
    upload: "Wysyłanie",
    lag: "Lagi pod obciążeniem",
    idle: "bez obciążenia",
    grade: "Twoja ocena",
    share: "Udostępnij wynik",
    saved: "Zapisano obraz",
    server: "Serwer testowy: {server}",
    failed: "Test nie połączył się z M-Lab. Sprawdź połączenie albo spróbuj za minutę.",
    verdictGood: "Twoje łącze jest w dobrej formie",
    verdictBloat: "Łącze jest szybkie, ale laguje, gdy ktoś coś pobiera",
    verdictSlow: "Łącze jest za wolne na wygodne oglądanie i rozmowy",
    verdictBoth: "Łącze jest wolne i laguje pod obciążeniem",
    tipBloat: "Opóźnienie rośnie z {idle} ms do {loaded} ms pod obciążeniem. Włącz w routerze SQM, Smart Queue albo Adaptive QoS i ustaw około 90% prędkości łącza.",
    tipSlow: "Zmierzono {down} Mb/s. Sprawdź swój pakiet i powtórz test po kablu, żeby wykluczyć Wi-Fi.",
    tipWifi: "Jeśli dalej coś się tnie, winne jest zwykle Wi-Fi: słaby sygnał, zatłoczony kanał albo router w złym miejscu. Przeglądarka tego nie widzi, WhyLag tak.",
    appTitle: "Sprawdź dokładnie, gdzie się psuje",
    appBody: "WhyLag na Windows, macOS i Linux sprawdza Twoje Wi-Fi, mapuje zasięg pokój po pokoju, znajduje każde urządzenie w sieci, śledzi ruch krok po kroku i mówi, czy winny jest router, czy operator.",
    appCta: "Pobierz WhyLag",
    appFree: "Za darmo",
    footer: "Prędkość mierzona z M-Lab (measurementlab.net).",
    shareText: "Mój internet dostał {grade} w WhyLag. A Twój?",
  },
} as const;

export type Key = keyof (typeof STRINGS)["en"];

export function detectLang(): Lang {
  const q = new URLSearchParams(location.search).get("lang");
  if (q === "pl" || q === "en") return q;
  return navigator.language?.toLowerCase().startsWith("pl") ? "pl" : "en";
}

export function tr(lang: Lang, key: Key, vars: Record<string, string | number> = {}) {
  let s: string = STRINGS[lang][key];
  for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
}
