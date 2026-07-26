import pandas as pd

# 1. 데이터 로드
air = pd.read_csv("examples/seoul_air_2026.csv")
stations = pd.read_csv("examples/seoul_station_info.csv")

# 타입 개념 대응 (xazz의 Option<float> → NaN 처리)
air["pm25"] = air["pm25"].fillna(0.0)
air["pm10"] = air["pm10"].fillna(0.0)

# 2. PM2.5 / PM10 비율 생성
air_with_ratio = air[
    (air["pm25"] > 0) &
    (air["pm10"] > 0)
].copy()

air_with_ratio["pm_ratio"] = air_with_ratio["pm25"] / air_with_ratio["pm10"]

# 3. 측정소별 평균 PM2.5 계산
station_pm25 = (
    air_with_ratio[air_with_ratio["station"] != "평균"]
    .groupby("station", as_index=False)["pm25"]
    .mean()
)

# 4. 지역 정보 조인
enriched = station_pm25.merge(stations, on="station", how="inner")

# 5. 인구 대비 미세먼지 지표 생성
enriched["pm25_per_1000"] = (
    enriched["pm25"] / enriched["population"] * 1000
)

final_result = (
    enriched.sort_values("pm25_per_1000", ascending=False)
    .head(10)
)

# 6. 시각화
import matplotlib.pyplot as plt

plt.figure()
plt.bar(final_result["district"], final_result["pm25_per_1000"])
plt.title("인구 대비 PM2.5 농도 상위 10개 지역")
plt.xlabel("district")
plt.ylabel("pm25_per_1000")
plt.xticks(rotation=45)
plt.tight_layout()
plt.show()