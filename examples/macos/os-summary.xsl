<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="text" encoding="UTF-8"/>
  <xsl:template match="/">
    <xsl:for-each select="//INSTANCE">
      <xsl:text>Caption=</xsl:text>
      <xsl:value-of select="PROPERTY[@NAME='Caption']/VALUE"/>
      <xsl:text>&#10;Version=</xsl:text>
      <xsl:value-of select="PROPERTY[@NAME='Version']/VALUE"/>
      <xsl:text>&#10;</xsl:text>
    </xsl:for-each>
  </xsl:template>
</xsl:stylesheet>
