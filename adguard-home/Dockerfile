FROM adguard/adguardhome

RUN mkdir -p /data/{conf,work} && \
    ln -s /data/conf /opt/adguardhome/conf && \
    ln -s /data/work /opt/adguardhome/work

COPY ./AdGuardHome.yaml /opt/adguardhome/conf/AdGuardHome.yaml
