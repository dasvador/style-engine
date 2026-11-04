import { Link, useParams } from 'react-router-dom';
import { ApiError } from '../api/client';
import { api } from '../api/endpoints';
import { useAsync } from '../hooks/useAsync';
import { useProtectedImage } from '../hooks/useProtectedImage';
import { REACTION_LABEL, WEAR_STATUS_LABEL } from '../lib/outfitRecord';
import type { WearDetail } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { ErrorMessage, Loading } from '../components/Status';

/** 사진이 없는 이유. 다른 사진으로 대신하지 않는다. */
function imageNote(detail: WearDetail): string {
  if (detail.image.status === 'file_missing') return '고정한 사진 파일을 찾을 수 없어요. 다른 사진으로 대신하지 않았어요.';
  if (detail.look_status === 'deleted') return '룩북에서 지운 코디라 사진이 없어요. 저장된 옷 구성만 보여 드려요.';
  if (detail.look_status === 'linked') return '이 코디의 룩에 아직 사진이 없어요.';
  return '이 기록에 연결된 룩이 없어요. 저장된 옷 구성만 보여 드려요.';
}

/** 사진이 언제 붙었는지. 저장 당시 사진이 아닐 수 있으면 그렇다고 밝힌다. */
function basisNote(detail: WearDetail): string | null {
  if (detail.image.basis === 'later') return '저장한 뒤 이 코디의 룩에 처음 만들어진 사진이에요.';
  if (detail.image.basis === 'migrated') return '예전 기록에 나중에 연결한 사진이에요. 저장 당시 사진인지는 확인되지 않아요.';
  if (detail.look_status === 'deleted') return '룩북에서는 지웠지만, 저장할 때 고정한 사진이에요.';
  return null;
}

function detailError(err: unknown): string {
  if (err instanceof ApiError && err.status === 404) {
    // 기록이 없을 때는 code 가 온다. code 가 없으면 서버가 이 경로를 아직 모르는 것이다.
    return err.body.includes('"code"')
      ? '이 코디 기록을 찾을 수 없어요.'
      : '서버를 다시 시작해야 코디를 볼 수 있어요.';
  }
  return '코디를 불러오지 못했어요. 잠시 뒤 다시 시도해 주세요.';
}

/**
 * 기록의 코디 한 벌. 저장된 옷 구성과 저장 때 고정한 사진을 보여 준다.
 * 주소만으로 열린다(새로고침·직접 접근). 사진을 새로 만들지 않으므로 `OutfitImage` 를 쓰지 않는다.
 * 토큰 계정도 볼 수 있게 사진은 소유자 확인을 거치는 보호 경로에서 읽는다.
 */
export function WearDetailPage() {
  const { recordId = '' } = useParams();
  const detail = useAsync<WearDetail>(
    () =>
      api.outfits.detail(recordId).catch((err: unknown) => {
        throw new Error(detailError(err));
      }),
    [recordId],
  );
  const data = detail.data;
  const record = data?.record;
  const image = useProtectedImage(data?.image.file ?? null);

  return (
    <>
      <ScreenHeader title={data?.look?.title ?? '저장된 코디'} sub={record ? record.date : undefined} />
      <Link className="link-action wear-open" to="/outfits">
        ← 코디 기록으로
      </Link>

      {detail.loading && !data && <Loading>코디를 불러오는 중...</Loading>}
      {detail.error && !data && <ErrorMessage>{detail.error}</ErrorMessage>}

      {data && record && (
        <article className={`wear-card ${record.status}`}>
          <header className="wear-card-head">
            <strong>
              {record.date}
              {[record.occasion, record.temperature !== null ? `${record.temperature}°C 기준` : null]
                .filter(Boolean)
                .map((t) => ` · ${t}`)
                .join('')}
            </strong>
            <span className={`wear-status ${record.status}`}>{WEAR_STATUS_LABEL[record.status]}</span>
          </header>

          {data.image.file && image.status !== 'failed' ? (
            <div className="lookbook-img-wrap wear-detail-image">
              <div className="lookbook-img-slot">
                {image.status === 'ready' ? (
                  <img
                    src={image.url}
                    alt={data.image.presentation === 'flat_lay' ? '저장된 코디의 의류 배치' : '저장된 코디 착장'}
                  />
                ) : (
                  <span className="caption">사진을 불러오는 중...</span>
                )}
              </div>
              {data.image.presentation === 'flat_lay' && <p className="lookbook-img-note">의류 배치 이미지예요.</p>}
              {basisNote(data) && <p className="lookbook-img-note">{basisNote(data)}</p>}
            </div>
          ) : (
            <p className="wear-note">
              {data.image.file ? '사진을 불러오지 못했어요. 다른 사진으로 대신하지 않았어요.' : imageNote(data)}
            </p>
          )}

          <ul className="rec-pieces">
            {record.items.map((i) => (
              <li className="rec-piece" key={i.id}>
                <span className="rec-piece-slot">{i.category}</span>
                <span className="rec-piece-name">
                  {i.name}
                  {data.removed_item_ids.includes(i.id) && <span className="wear-removed">지금 옷장에 없어요</span>}
                </span>
              </li>
            ))}
          </ul>

          {record.reaction && <p className="wear-note">반응: {REACTION_LABEL[record.reaction]}</p>}
          {record.comment && <p className="wear-note">메모: {record.comment}</p>}
        </article>
      )}
    </>
  );
}
