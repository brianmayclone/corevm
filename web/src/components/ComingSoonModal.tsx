import { motion, AnimatePresence } from 'framer-motion';
import { X, Rocket, AlertTriangle } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface ComingSoonModalProps {
  open: boolean;
  onClose: () => void;
  lang: Lang;
  variant?: 'coming-soon' | 'download-error';
}

export default function ComingSoonModal({ open, onClose, lang, variant = 'coming-soon' }: ComingSoonModalProps) {
  const isError = variant === 'download-error';
  const titleKey = isError ? 'download_error_title' : 'coming_soon_title';
  const descKey = isError ? 'download_error_desc' : 'coming_soon_desc';
  const hintKey = isError ? 'download_error_hint' : 'coming_soon_hint';
  const closeKey = isError ? 'download_error_close' : 'coming_soon_close';
  const Icon = isError ? AlertTriangle : Rocket;
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="fixed inset-0 z-[100] flex items-center justify-center px-4"
          onClick={onClose}
        >
          {/* Backdrop */}
          <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />

          {/* Modal */}
          <motion.div
            initial={{ opacity: 0, scale: 0.9, y: 20 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.9, y: 20 }}
            transition={{ type: 'spring', duration: 0.4 }}
            className="relative w-full max-w-md rounded-2xl border border-white/10 bg-surface-900 p-8 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Close button */}
            <button
              onClick={onClose}
              className="absolute top-4 right-4 rounded-lg p-1.5 text-surface-500 transition-colors hover:bg-white/5 hover:text-white cursor-pointer bg-transparent border-none"
            >
              <X size={18} />
            </button>

            {/* Content */}
            <div className="text-center">
              <div className={`mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-full ${isError ? 'bg-red-500/15' : 'bg-primary-500/15'}`}>
                <Icon size={28} className={isError ? 'text-red-400' : 'text-primary-400'} />
              </div>
              <h3 className="text-xl font-bold text-white">
                {t(lang, titleKey)}
              </h3>
              <p className="mt-3 text-sm leading-relaxed text-surface-400">
                {t(lang, descKey)}
              </p>
              <div className="mt-4 text-xs text-surface-500">
                {t(lang, hintKey)}
              </div>
              <button
                onClick={onClose}
                className="mt-6 rounded-xl bg-primary-500 px-6 py-2.5 text-sm font-semibold text-white transition-all hover:bg-primary-400 cursor-pointer border-none"
              >
                {t(lang, closeKey)}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
