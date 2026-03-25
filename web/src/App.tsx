import { useState } from 'react';
import { getInitialLang, persistLang } from './i18n';
import type { Lang } from './i18n';
import Navbar from './components/Navbar';
import Hero from './components/Hero';
import Appliance from './components/Appliance';
import Features from './components/Features';
import Screenshots from './components/Screenshots';
import Cluster from './components/Cluster';
import Architecture from './components/Architecture';
import CTA from './components/CTA';
import Footer from './components/Footer';

export default function App() {
  const [lang, setLang] = useState<Lang>(getInitialLang);

  const handleLangChange = (newLang: Lang) => {
    setLang(newLang);
    persistLang(newLang);
    document.documentElement.lang = newLang;
  };

  return (
    <div className="min-h-screen bg-surface-950 text-white">
      <Navbar lang={lang} onLangChange={handleLangChange} />
      <main>
        <Hero lang={lang} />
        <Appliance lang={lang} />
        <Features lang={lang} />
        <Screenshots lang={lang} />
        <Cluster lang={lang} />
        <Architecture lang={lang} />
        <CTA lang={lang} />
      </main>
      <Footer lang={lang} />
    </div>
  );
}
