-- 여성 sporty_casual + vintage 아이템 시드 (각 15개, 총 30개)

INSERT INTO clothing (id, name, category, gender, style_mood, color, thickness, material_primary, sub_category, texture_keywords, tone, style, role)
VALUES
-- ===== sporty_casual =====
(UUID(), '블랙 나일론 윈드브레이커', '아우터', 'female', 'sporty_casual', 'black', '얇은', 'nylon', '윈드브레이커', 'nylon, lightweight, windproof', '어두운', 'sporty', 'outer'),
(UUID(), '라이트그레이 코튼 크롭 후드집업', '아우터', 'female', 'sporty_casual', 'light_gray', '중간', 'cotton', '후드집업', 'french terry, cropped zip-up', '밝은', 'sporty', 'outer'),
(UUID(), '크림 메쉬 패널 트랙재킷', '아우터', 'female', 'sporty_casual', 'cream', '얇은', 'polyester', '트랙재킷', 'polyester, mesh panel, track', '밝은', 'sporty', 'outer'),
(UUID(), '화이트 드라이핏 오버사이즈 반팔티', '상의', 'female', 'sporty_casual', 'white', '얇은', 'polyester', '반팔티셔츠', 'dry-fit, moisture wicking, oversized', '밝은', 'sporty', 'base'),
(UUID(), '블랙 래글런 소매 긴팔 탑', '상의', 'female', 'sporty_casual', 'black', '중간', 'nylon', '긴팔티셔츠', 'nylon, raglan sleeve, fitted', '어두운', 'sporty', 'base'),
(UUID(), '네이비 스트라이프 카라 크롭 폴로셔츠', '상의', 'female', 'sporty_casual', 'navy', '중간', 'cotton', '폴로셔츠', 'cotton pique, cropped polo, stripe', '어두운', 'sporty', 'base'),
(UUID(), '차콜 스웨트 와이드 조거팬츠', '하의', 'female', 'sporty_casual', 'charcoal', '중간', 'cotton', '조거팬츠', 'french terry, wide jogger, elastic cuff', '어두운', 'sporty', 'base'),
(UUID(), '블랙 스판 하이웨이스트 레깅스', '하의', 'female', 'sporty_casual', 'black', '얇은', 'spandex', '레깅스', 'spandex, high waist, compression', '어두운', 'sporty', 'base'),
(UUID(), '카키 나일론 카고 와이드팬츠', '하의', 'female', 'sporty_casual', 'khaki', '중간', 'nylon', '카고팬츠', 'nylon, cargo pockets, wide leg', '중간', 'sporty', 'base'),
(UUID(), '화이트 메쉬 러닝 스니커즈', '신발', 'female', 'sporty_casual', 'white', '중간', 'mesh', '러닝화', 'mesh, lightweight runner', '밝은', 'sporty', 'base'),
(UUID(), '블랙 가죽 청키 플랫폼 스니커즈', '신발', 'female', 'sporty_casual', 'black', '두꺼운', 'leather', '스니커즈', 'leather, chunky platform sole', '어두운', 'sporty', 'base'),
(UUID(), '실버 나일론 스트랩 테크 샌들', '신발', 'female', 'sporty_casual', 'silver', '중간', 'nylon', '스포츠샌들', 'nylon strap, tech sandal', '밝은', 'sporty', 'base'),
(UUID(), '블랙 나일론 미니 크로스백', '가방', 'female', 'sporty_casual', 'black', '얇은', 'nylon', '크로스백', 'nylon, mini crossbody', '어두운', 'sporty', 'accent'),
(UUID(), '올리브 코듀라 슬링백', '가방', 'female', 'sporty_casual', 'olive', '중간', 'cordura', '슬링백', 'cordura, crossbody sling', '어두운', 'sporty', 'accent'),
(UUID(), '베이지 리플스탑 미니 백팩', '가방', 'female', 'sporty_casual', 'beige', '얇은', 'nylon', '백팩', 'ripstop nylon, mini backpack', '밝은', 'sporty', 'accent'),

-- ===== vintage =====
(UUID(), '브라운 코듀로이 오버핏 셔츠', '상의', 'female', 'vintage', 'brown', '중간', 'corduroy', '셔츠', 'corduroy, oversized, retro', '어두운', 'vintage', 'base'),
(UUID(), '크림 레이스 트림 긴팔 블라우스', '상의', 'female', 'vintage', 'cream', '얇은', 'cotton', '블라우스', 'cotton, lace trim, romantic', '밝은', 'vintage', 'base'),
(UUID(), '버건디 벨벳 스모크 크롭탑', '상의', 'female', 'vintage', 'burgundy', '중간', 'velvet', '크롭탑', 'velvet, smocked, cropped', '어두운', 'vintage', 'base'),
(UUID(), '머스타드 레트로 프린트 카디건', '아우터', 'female', 'vintage', 'mustard', '중간', 'acrylic', '카디건', 'acrylic knit, retro pattern', '밝은', 'vintage', 'accent'),
(UUID(), '카멜 스웨이드 크롭 트러커재킷', '아우터', 'female', 'vintage', 'camel', '중간', 'suede', '트러커재킷', 'suede, cropped trucker', '중간', 'vintage', 'outer'),
(UUID(), '워싱 인디고 데님 오버사이즈 재킷', '아우터', 'female', 'vintage', 'indigo', '중간', 'denim', '데님재킷', 'washed denim, oversized, 90s', '어두운', 'vintage', 'outer'),
(UUID(), '브라운 코듀로이 와이드 팬츠', '하의', 'female', 'vintage', 'brown', '중간', 'corduroy', '와이드팬츠', 'corduroy, wide leg, retro', '어두운', 'vintage', 'base'),
(UUID(), '워싱 블루 하이웨이스트 플레어 데님', '하의', 'female', 'vintage', 'blue', '중간', 'denim', '플레어진', 'washed denim, high waist, flare, 70s', '중간', 'vintage', 'base'),
(UUID(), '아이보리 플로럴 미디 플리츠스커트', '하의', 'female', 'vintage', 'ivory', '얇은', 'polyester', '미디스커트', 'floral print, pleated, midi', '밝은', 'vintage', 'base'),
(UUID(), '브라운 레더 라운드토 메리제인', '신발', 'female', 'vintage', 'brown', '중간', 'leather', '메리제인', 'leather, round toe, vintage strap', '어두운', 'vintage', 'base'),
(UUID(), '버건디 스웨이드 청키힐 로퍼', '신발', 'female', 'vintage', 'burgundy', '중간', 'suede', '로퍼', 'suede, chunky heel, retro', '어두운', 'vintage', 'base'),
(UUID(), '오프화이트 캔버스 로우 스니커즈', '신발', 'female', 'vintage', 'off_white', '중간', 'canvas', '캔버스스니커즈', 'canvas, low top, classic', '밝은', 'vintage', 'base'),
(UUID(), '탄 레더 버클 빈티지 숄더백', '가방', 'female', 'vintage', 'tan', '중간', 'leather', '숄더백', 'leather, buckle, vintage', '중간', 'vintage', 'accent'),
(UUID(), '올리브 캔버스 크로스 사첼백', '가방', 'female', 'vintage', 'olive', '중간', 'canvas', '사첼백', 'canvas, vintage satchel', '어두운', 'vintage', 'accent'),
(UUID(), '브라운 우븐 라탄 미니 토트백', '가방', 'female', 'vintage', 'brown', '중간', 'rattan', '토트백', 'woven rattan, mini tote', '중간', 'vintage', 'accent');
