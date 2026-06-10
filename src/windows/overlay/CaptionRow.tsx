import { memo } from "react";
import type { Caption } from "../../lib/types";

interface Props {
  caption: Caption;
}

function CaptionRow({ caption }: Props) {
  const bilingual = caption.translation !== undefined;
  const streaming = bilingual && !caption.translationDone;

  return (
    <div className={`caption ${caption.isFinal ? "final" : "partial"}`}>
      <span className={`accent ${caption.source}`} />
      <div className="caption-body">
        {bilingual ? (
          <>
            <div className="original">{caption.original}</div>
            <div className="main-text">
              {caption.translation}
              {streaming && <span className="caret" />}
            </div>
          </>
        ) : (
          <div className="main-text">
            {caption.original}
            {!caption.isFinal && <span className="caret" />}
          </div>
        )}
      </div>
    </div>
  );
}

export default memo(CaptionRow);
