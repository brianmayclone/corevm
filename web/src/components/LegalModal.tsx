import { motion, AnimatePresence } from 'framer-motion';
import { X } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

export type LegalPage = 'imprint' | 'privacy' | null;

interface LegalModalProps {
  page: LegalPage;
  onClose: () => void;
  lang: Lang;
}

function Imprint({ lang }: { lang: Lang }) {
  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-white">{t(lang, 'legal_imprint')}</h2>

      <div className="space-y-4 text-sm leading-relaxed text-surface-300">
        <p className="text-xs uppercase tracking-wider text-surface-500">
          {t(lang, 'legal_responsible')}
        </p>

        <div className="grid gap-6 sm:grid-cols-2">
          <div className="rounded-xl border border-white/5 bg-white/[0.02] p-5">
            <p className="font-semibold text-white">Mike Strathmann</p>
            <p className="mt-2">Waltalingerstrasse 2</p>
            <p>8526 Niederneunforn</p>
            <p>{t(lang, 'legal_country_ch')}</p>
            <p className="mt-2">Tel: +41 76 492 50 73</p>
          </div>

          <div className="rounded-xl border border-white/5 bg-white/[0.02] p-5">
            <p className="font-semibold text-white">Christian Möller</p>
            <p className="mt-2">Werbiger Weg 9</p>
            <p>15234 Frankfurt (Oder)</p>
            <p>{t(lang, 'legal_country_de')}</p>
          </div>
        </div>

        <div>
          <p className="text-xs uppercase tracking-wider text-surface-500 mb-2">
            {t(lang, 'legal_contact')}
          </p>
          <p>E-Mail: info@corevm.de</p>
        </div>

        <div>
          <p className="text-xs uppercase tracking-wider text-surface-500 mb-2">
            {t(lang, 'legal_disclaimer_title')}
          </p>
          <p>{t(lang, 'legal_disclaimer_text')}</p>
        </div>
      </div>
    </div>
  );
}

function Privacy({ lang }: { lang: Lang }) {
  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-white">{t(lang, 'legal_privacy')}</h2>

      <div className="space-y-5 text-sm leading-relaxed text-surface-300">
        <div>
          <h3 className="mb-2 text-base font-semibold text-white">{t(lang, 'privacy_responsible_title')}</h3>
          <p>{t(lang, 'privacy_responsible_text')}</p>
        </div>

        <div>
          <h3 className="mb-2 text-base font-semibold text-white">{t(lang, 'privacy_hosting_title')}</h3>
          <p>{t(lang, 'privacy_hosting_text')}</p>
        </div>

        <div>
          <h3 className="mb-2 text-base font-semibold text-white">{t(lang, 'privacy_cookies_title')}</h3>
          <p>{t(lang, 'privacy_cookies_text')}</p>
        </div>

        <div>
          <h3 className="mb-2 text-base font-semibold text-white">{t(lang, 'privacy_localstorage_title')}</h3>
          <p>{t(lang, 'privacy_localstorage_text')}</p>
        </div>

        <div>
          <h3 className="mb-2 text-base font-semibold text-white">{t(lang, 'privacy_rights_title')}</h3>
          <p>{t(lang, 'privacy_rights_text')}</p>
        </div>
      </div>
    </div>
  );
}

export default function LegalModal({ page, onClose, lang }: LegalModalProps) {
  return (
    <AnimatePresence>
      {page && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="fixed inset-0 z-[100] flex items-start justify-center overflow-y-auto px-4 py-12"
          onClick={onClose}
        >
          {/* Backdrop */}
          <div className="fixed inset-0 bg-black/60 backdrop-blur-sm" />

          {/* Modal */}
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: 20 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 20 }}
            transition={{ type: 'spring', duration: 0.4 }}
            className="relative w-full max-w-2xl rounded-2xl border border-white/10 bg-surface-900 p-8 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Close */}
            <button
              onClick={onClose}
              className="absolute top-4 right-4 rounded-lg p-1.5 text-surface-500 transition-colors hover:bg-white/5 hover:text-white cursor-pointer bg-transparent border-none"
            >
              <X size={18} />
            </button>

            {page === 'imprint' ? <Imprint lang={lang} /> : <Privacy lang={lang} />}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
